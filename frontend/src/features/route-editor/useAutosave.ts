import { useEffect, useCallback, useRef, useState } from "react";
import type { PendingOperation, RouteOperation } from "./types";

const DB_NAME = "haiker-route-editor";
const DB_VERSION = 1;
const STORE_NAME = "pending-operations";

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        const store = db.createObjectStore(STORE_NAME, { keyPath: "id" });
        store.createIndex("draftId", "draftId", { unique: false });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function savePendingOperation(op: PendingOperation): Promise<void> {
  const db = await openDatabase();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    tx.objectStore(STORE_NAME).put(op);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

async function confirmOperation(id: string): Promise<void> {
  const db = await openDatabase();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const getReq = store.get(id);
    getReq.onsuccess = () => {
      const record = getReq.result as PendingOperation | undefined;
      if (record) {
        record.confirmed = true;
        store.put(record);
      }
    };
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

async function getUnconfirmedOperations(
  draftId: string,
): Promise<PendingOperation[]> {
  const db = await openDatabase();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readonly");
    const store = tx.objectStore(STORE_NAME);
    const index = store.index("draftId");
    const request = index.getAll(draftId);
    request.onsuccess = () => {
      const all = request.result as PendingOperation[];
      resolve(all.filter((op) => !op.confirmed));
    };
    request.onerror = () => reject(request.error);
  });
}

async function clearOperationsForDraft(draftId: string): Promise<void> {
  const db = await openDatabase();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const index = store.index("draftId");
    const request = index.getAllKeys(draftId);
    request.onsuccess = () => {
      for (const key of request.result) {
        store.delete(key);
      }
    };
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

interface UseAutosaveOptions {
  draftId: string | null;
  onRecoveryAvailable?: (operations: PendingOperation[]) => void;
}

export function useAutosave({ draftId, onRecoveryAvailable }: UseAutosaveOptions) {
  const [hasRecovery, setHasRecovery] = useState(false);
  const [recoveryOps, setRecoveryOps] = useState<PendingOperation[]>([]);
  const checkedRef = useRef(false);

  // Check for unconfirmed operations on mount
  useEffect(() => {
    if (!draftId || checkedRef.current) return;
    checkedRef.current = true;

    void getUnconfirmedOperations(draftId).then((ops) => {
      if (ops.length > 0) {
        setHasRecovery(true);
        setRecoveryOps(ops);
        onRecoveryAvailable?.(ops);
      }
    });
  }, [draftId, onRecoveryAvailable]);

  const saveOperation = useCallback(
    async (
      operation: RouteOperation,
      expectedRevision: number,
    ): Promise<string> => {
      const id = crypto.randomUUID();
      if (!draftId) return id;

      const pending: PendingOperation = {
        id,
        draftId,
        operation,
        expectedRevision,
        timestamp: Date.now(),
        confirmed: false,
      };
      await savePendingOperation(pending);
      return id;
    },
    [draftId],
  );

  const confirmSaved = useCallback(async (operationId: string) => {
    await confirmOperation(operationId);
  }, []);

  const clearRecovery = useCallback(async () => {
    if (!draftId) return;
    await clearOperationsForDraft(draftId);
    setHasRecovery(false);
    setRecoveryOps([]);
  }, [draftId]);

  const dismissRecovery = useCallback(() => {
    setHasRecovery(false);
    setRecoveryOps([]);
    if (draftId) {
      void clearOperationsForDraft(draftId);
    }
  }, [draftId]);

  const getUnconfirmedOps = useCallback(async (): Promise<PendingOperation[]> => {
    if (!draftId) return [];
    return getUnconfirmedOperations(draftId);
  }, [draftId]);

  return {
    saveOperation,
    confirmSaved,
    clearRecovery,
    hasRecovery,
    recoveryOps,
    dismissRecovery,
    getUnconfirmedOps,
  };
}
