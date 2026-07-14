import { useState, useCallback, useEffect, useRef } from "react";
import { useQuery, useMutation } from "@tanstack/react-query";
import {
  startImport,
  completeUpload,
  getImportStatus,
  ApiError,
  type StartImportResponse,
  type ImportStatusResponse,
} from "@/api/client";

const TERMINAL_STATUSES = new Set(["completed", "failed", "cancelled"]);

function isTerminalStatus(status: string): boolean {
  return TERMINAL_STATUSES.has(status);
}

async function computeSha256Hex(buffer: ArrayBuffer): Promise<string> {
  const hashBuffer = await crypto.subtle.digest("SHA-256", buffer);
  const hashArray = new Uint8Array(hashBuffer);
  return Array.from(hashArray)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

function uploadFileWithProgress(
  url: string,
  file: File,
  onProgress: (percent: number) => void,
  signal: AbortSignal,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();

    signal.addEventListener("abort", () => {
      xhr.abort();
    });

    xhr.upload.addEventListener("progress", (event) => {
      if (event.lengthComputable) {
        const percent = Math.round((event.loaded / event.total) * 100);
        onProgress(percent);
      }
    });

    xhr.addEventListener("load", () => {
      if (xhr.status >= 200 && xhr.status < 300) {
        resolve();
      } else {
        reject(new Error(`Upload failed with status ${xhr.status}`));
      }
    });

    xhr.addEventListener("error", () => {
      reject(new Error("Upload network error"));
    });

    xhr.addEventListener("abort", () => {
      reject(new Error("Upload aborted"));
    });

    xhr.open("PUT", url);
    xhr.setRequestHeader("Content-Type", file.type || "application/octet-stream");
    xhr.send(file);
  });
}

export type ImportPhase =
  | "idle"
  | "starting"
  | "uploading"
  | "completing"
  | "processing"
  | "completed"
  | "failed"
  | "duplicate";

export interface UseImportActivityResult {
  phase: ImportPhase;
  uploadProgress: number;
  importStatus: ImportStatusResponse | null;
  importId: string | null;
  error: string | null;
  duplicateActivityId: string | null;
  startFileImport: (file: File) => void;
  reset: () => void;
}

export function useImportActivity(
  initialImportId: string | null,
  onImportIdChange: (id: string | null) => void,
): UseImportActivityResult {
  const [importId, setImportId] = useState<string | null>(initialImportId);
  const [phase, setPhase] = useState<ImportPhase>(
    initialImportId ? "processing" : "idle",
  );
  const [uploadProgress, setUploadProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [duplicateActivityId, setDuplicateActivityId] = useState<string | null>(
    null,
  );
  const abortControllerRef = useRef<AbortController | null>(null);

  const updateImportId = useCallback(
    (id: string | null) => {
      setImportId(id);
      onImportIdChange(id);
    },
    [onImportIdChange],
  );

  // Polling for import status
  const importStatusQuery = useQuery<ImportStatusResponse>({
    queryKey: ["importStatus", importId],
    queryFn: () => getImportStatus(importId!),
    enabled: !!importId && (phase === "processing" || phase === "completing"),
    refetchInterval: (query) => {
      const status = query.state.data?.status;
      if (status && isTerminalStatus(status)) return false;
      return 1500;
    },
  });

  // Transition to terminal states based on polling result
  useEffect(() => {
    if (!importStatusQuery.data) return;
    const status = importStatusQuery.data.status;
    if (status === "completed") {
      setPhase("completed");
    } else if (status === "failed") {
      setPhase("failed");
      setError(
        importStatusQuery.data.failureReason ?? "Import failed unexpectedly",
      );
    } else if (status === "cancelled") {
      setPhase("failed");
      setError("Import was cancelled");
    } else if (phase !== "uploading" && phase !== "starting" && phase !== "completing") {
      setPhase("processing");
    }
  }, [importStatusQuery.data, phase]);

  // Start import mutation
  const startImportMutation = useMutation<
    StartImportResponse,
    Error,
    { file: File }
  >({
    mutationFn: async ({ file }) => {
      const contentType = file.name.toLowerCase().endsWith(".gpx")
        ? ("application/gpx+xml" as const)
        : ("application/xml" as const);
      return startImport(
        {
          filename: file.name,
          contentType,
          fileSizeBytes: file.size,
        },
        crypto.randomUUID(),
      );
    },
    onSuccess: async (data, { file }) => {
      updateImportId(data.importId);
      setPhase("uploading");
      setUploadProgress(0);

      // Start upload
      const controller = new AbortController();
      abortControllerRef.current = controller;

      try {
        await uploadFileWithProgress(
          data.uploadUrl,
          file,
          setUploadProgress,
          controller.signal,
        );

        setPhase("completing");
        // Compute checksum and complete
        const buffer = await file.arrayBuffer();
        const checksum = await computeSha256Hex(buffer);
        await completeUpload(data.importId, checksum);
        setPhase("processing");
      } catch (err) {
        if (controller.signal.aborted) return;
        setPhase("failed");
        setError(
          err instanceof Error ? err.message : "Upload failed unexpectedly",
        );
      }
    },
    onError: (err) => {
      if (err instanceof ApiError && err.status === 409) {
        // Duplicate detected at start
        setPhase("duplicate");
        setDuplicateActivityId(null);
        setError("This file has already been imported");
        return;
      }
      setPhase("failed");
      setError(err instanceof Error ? err.message : "Failed to start import");
    },
  });

  const startFileImport = useCallback(
    (file: File) => {
      setError(null);
      setDuplicateActivityId(null);
      setPhase("starting");
      startImportMutation.mutate({ file });
    },
    [startImportMutation],
  );

  const reset = useCallback(() => {
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
    setPhase("idle");
    setUploadProgress(0);
    setError(null);
    setDuplicateActivityId(null);
    updateImportId(null);
  }, [updateImportId]);

  // Determine duplicate from import status
  useEffect(() => {
    if (
      importStatusQuery.data?.status === "failed" &&
      importStatusQuery.data.failureReason?.toLowerCase().includes("duplicate")
    ) {
      setPhase("duplicate");
      setDuplicateActivityId(importStatusQuery.data.activityId ?? null);
    }
  }, [importStatusQuery.data]);

  // beforeunload during upload
  useEffect(() => {
    if (phase !== "uploading" && phase !== "completing") return;
    const handler = (e: BeforeUnloadEvent) => {
      e.preventDefault();
    };
    window.addEventListener("beforeunload", handler);
    return () => {
      window.removeEventListener("beforeunload", handler);
    };
  }, [phase]);

  return {
    phase,
    uploadProgress,
    importStatus: importStatusQuery.data ?? null,
    importId,
    error,
    duplicateActivityId,
    startFileImport,
    reset,
  };
}
