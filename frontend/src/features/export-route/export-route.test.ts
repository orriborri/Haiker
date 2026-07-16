import { describe, it, expect } from "vitest";
import {
  exportReducer,
  initialExportState,
  isTerminalStatus,
  getProgressLabel,
  TERMINAL_STATUSES,
  type ExportState,
  type ExportAction,
  type ExportPhase,
} from "./useExportRoute";

/**
 * Helper to dispatch a sequence of actions and return the final state.
 */
function applyActions(
  state: ExportState,
  actions: ExportAction[],
): ExportState {
  return actions.reduce((s, a) => exportReducer(s, a), state);
}

describe("ExportPhase type coverage", () => {
  it("includes all required phase values", () => {
    const phases: ExportPhase[] = [
      "idle",
      "requesting",
      "polling",
      "ready",
      "failed",
      "expired",
      "offline",
      "unauthorized",
    ];
    // Verify each is a valid phase by assigning to a typed variable
    phases.forEach((phase) => {
      const p: ExportPhase = phase;
      expect(p).toBe(phase);
    });
  });
});

describe("Terminal status detection", () => {
  it("ready, failed, and expired are terminal statuses", () => {
    expect(isTerminalStatus("ready")).toBe(true);
    expect(isTerminalStatus("failed")).toBe(true);
    expect(isTerminalStatus("expired")).toBe(true);
  });

  it("queued and generating are not terminal statuses", () => {
    expect(isTerminalStatus("queued")).toBe(false);
    expect(isTerminalStatus("generating")).toBe(false);
  });

  it("TERMINAL_STATUSES set contains exactly three entries", () => {
    expect(TERMINAL_STATUSES.size).toBe(3);
  });
});

describe("Status label mapping for progress display", () => {
  it("returns correct label for queued status", () => {
    expect(getProgressLabel("queued")).toBe("Queued for generation...");
  });

  it("returns correct label for generating status", () => {
    expect(getProgressLabel("generating")).toBe("Generating GPX...");
  });

  it("returns fallback for other statuses", () => {
    expect(getProgressLabel("ready")).toBe("Processing...");
    expect(getProgressLabel("failed")).toBe("Processing...");
    expect(getProgressLabel("expired")).toBe("Processing...");
  });
});

describe("Phase transitions: idle -> requesting -> polling -> ready", () => {
  it("START_REQUEST transitions from idle to requesting", () => {
    const state = exportReducer(initialExportState, { type: "START_REQUEST" });
    expect(state.phase).toBe("requesting");
    expect(state.error).toBeNull();
  });

  it("REQUEST_SUCCESS transitions from requesting to polling with exportId", () => {
    const state = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_SUCCESS", exportId: "export-123" },
    ]);
    expect(state.phase).toBe("polling");
    expect(state.exportId).toBe("export-123");
  });

  it("POLL_UPDATE with ready transitions to ready phase", () => {
    const state = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_SUCCESS", exportId: "export-123" },
      { type: "POLL_UPDATE", status: "ready" },
    ]);
    expect(state.phase).toBe("ready");
    expect(state.polledStatus).toBe("ready");
  });

  it("full journey: idle -> requesting -> polling (queued) -> polling (generating) -> ready", () => {
    const state = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_SUCCESS", exportId: "export-456" },
      { type: "POLL_UPDATE", status: "queued" },
      { type: "POLL_UPDATE", status: "generating" },
      { type: "POLL_UPDATE", status: "ready" },
    ]);
    expect(state.phase).toBe("ready");
    expect(state.exportId).toBe("export-456");
    expect(state.polledStatus).toBe("ready");
  });
});

describe("Phase transitions: polling -> failed", () => {
  it("POLL_UPDATE with failed transitions to failed phase with reason", () => {
    const state = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_SUCCESS", exportId: "export-123" },
      { type: "POLL_UPDATE", status: "failed", failureReason: "Server error" },
    ]);
    expect(state.phase).toBe("failed");
    expect(state.failureReason).toBe("Server error");
    expect(state.error).toBe("Server error");
  });

  it("POLL_UPDATE with failed uses default message when no reason given", () => {
    const state = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_SUCCESS", exportId: "export-123" },
      { type: "POLL_UPDATE", status: "failed", failureReason: null },
    ]);
    expect(state.phase).toBe("failed");
    expect(state.failureReason).toBe("Export failed unexpectedly");
  });
});

describe("Phase transitions: polling -> expired", () => {
  it("POLL_UPDATE with expired transitions to expired phase", () => {
    const state = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_SUCCESS", exportId: "export-123" },
      { type: "POLL_UPDATE", status: "expired" },
    ]);
    expect(state.phase).toBe("expired");
    expect(state.polledStatus).toBe("expired");
  });
});

describe("Expired state is distinct from failed state", () => {
  it("expired has phase 'expired' while failed has phase 'failed'", () => {
    const expiredState = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_SUCCESS", exportId: "export-1" },
      { type: "POLL_UPDATE", status: "expired" },
    ]);

    const failedState = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_SUCCESS", exportId: "export-2" },
      { type: "POLL_UPDATE", status: "failed", failureReason: "error" },
    ]);

    expect(expiredState.phase).toBe("expired");
    expect(failedState.phase).toBe("failed");
    expect(expiredState.phase).not.toBe(failedState.phase);
  });

  it("expired does not set failureReason", () => {
    const state = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_SUCCESS", exportId: "export-1" },
      { type: "POLL_UPDATE", status: "expired" },
    ]);
    expect(state.failureReason).toBeNull();
  });
});

describe("Duplicate request handling (409)", () => {
  it("REQUEST_DUPLICATE transitions to polling with existing exportId", () => {
    const state = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_DUPLICATE", exportId: "existing-export-789" },
    ]);
    expect(state.phase).toBe("polling");
    expect(state.exportId).toBe("existing-export-789");
  });
});

describe("Offline state behavior", () => {
  it("SET_OFFLINE(true) transitions idle to offline", () => {
    const state = exportReducer(initialExportState, {
      type: "SET_OFFLINE",
      isOffline: true,
    });
    expect(state.phase).toBe("offline");
  });

  it("SET_OFFLINE(false) transitions offline back to idle", () => {
    const state = applyActions(initialExportState, [
      { type: "SET_OFFLINE", isOffline: true },
      { type: "SET_OFFLINE", isOffline: false },
    ]);
    expect(state.phase).toBe("idle");
  });

  it("SET_OFFLINE(true) does not affect non-idle phases (e.g., polling)", () => {
    const pollingState: ExportState = {
      ...initialExportState,
      phase: "polling",
      exportId: "export-123",
    };
    const state = exportReducer(pollingState, {
      type: "SET_OFFLINE",
      isOffline: true,
    });
    expect(state.phase).toBe("polling");
  });
});

describe("Retry behavior", () => {
  it("RETRY resets to idle state", () => {
    const failedState = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_SUCCESS", exportId: "export-123" },
      { type: "POLL_UPDATE", status: "failed", failureReason: "timeout" },
    ]);
    expect(failedState.phase).toBe("failed");

    const state = exportReducer(failedState, { type: "RETRY" });
    expect(state.phase).toBe("idle");
    expect(state.exportId).toBeNull();
    expect(state.error).toBeNull();
    expect(state.failureReason).toBeNull();
    expect(state.polledStatus).toBeNull();
  });

  it("RETRY from expired also resets to idle", () => {
    const expiredState = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_SUCCESS", exportId: "export-123" },
      { type: "POLL_UPDATE", status: "expired" },
    ]);

    const state = exportReducer(expiredState, { type: "RETRY" });
    expect(state.phase).toBe("idle");
  });
});

describe("Resume polling on refresh", () => {
  it("RESUME_POLLING transitions to polling phase", () => {
    const state = exportReducer(initialExportState, {
      type: "RESUME_POLLING",
    });
    expect(state.phase).toBe("polling");
  });

  it("initialExportState with exportId already set can resume via RESUME_POLLING", () => {
    const stateWithExportId: ExportState = {
      ...initialExportState,
      exportId: "existing-export-from-url",
    };
    const state = exportReducer(stateWithExportId, {
      type: "RESUME_POLLING",
    });
    expect(state.phase).toBe("polling");
    expect(state.exportId).toBe("existing-export-from-url");
  });
});

describe("Unauthorized state", () => {
  it("REQUEST_UNAUTHORIZED transitions to unauthorized phase with error message", () => {
    const state = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_UNAUTHORIZED" },
    ]);
    expect(state.phase).toBe("unauthorized");
    expect(state.error).toContain("not authorized");
  });

  it("unauthorized is distinct from failed", () => {
    const unauthorizedState = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_UNAUTHORIZED" },
    ]);
    const failedState = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_FAILURE", error: "some error" },
    ]);
    expect(unauthorizedState.phase).toBe("unauthorized");
    expect(failedState.phase).toBe("failed");
    expect(unauthorizedState.phase).not.toBe(failedState.phase);
  });
});

describe("REQUEST_FAILURE transitions", () => {
  it("REQUEST_FAILURE transitions to failed with error message", () => {
    const state = applyActions(initialExportState, [
      { type: "START_REQUEST" },
      { type: "REQUEST_FAILURE", error: "Network error" },
    ]);
    expect(state.phase).toBe("failed");
    expect(state.error).toBe("Network error");
  });
});

describe("Initial state", () => {
  it("has sensible defaults", () => {
    expect(initialExportState.phase).toBe("idle");
    expect(initialExportState.exportId).toBeNull();
    expect(initialExportState.error).toBeNull();
    expect(initialExportState.polledStatus).toBeNull();
    expect(initialExportState.failureReason).toBeNull();
  });
});

describe("Polling status updates during polling phase", () => {
  it("POLL_UPDATE with queued keeps phase as polling", () => {
    const pollingState: ExportState = {
      ...initialExportState,
      phase: "polling",
      exportId: "export-123",
    };
    const state = exportReducer(pollingState, {
      type: "POLL_UPDATE",
      status: "queued",
    });
    expect(state.phase).toBe("polling");
    expect(state.polledStatus).toBe("queued");
  });

  it("POLL_UPDATE with generating keeps phase as polling", () => {
    const pollingState: ExportState = {
      ...initialExportState,
      phase: "polling",
      exportId: "export-123",
    };
    const state = exportReducer(pollingState, {
      type: "POLL_UPDATE",
      status: "generating",
    });
    expect(state.phase).toBe("polling");
    expect(state.polledStatus).toBe("generating");
  });
});
