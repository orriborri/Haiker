import { useCallback, useMemo } from "react";
import type { EditorState, EditorAction, SectionSelection, ReplaceSectionOperation, RouteOperation } from "./types";
import { polylineDistance, MAX_REPLACEMENT_POINTS } from "./geo-utils";

interface UseDrawReplacementSectionOptions {
  state: EditorState;
  dispatch: React.Dispatch<EditorAction>;
  dispatchOperation: (operation: RouteOperation) => Promise<void>;
}

interface UseDrawReplacementSectionResult {
  /** Whether drawing mode is currently active */
  isDrawing: boolean;
  /** Start drawing using the current section selection */
  startDrawing: (selection: SectionSelection) => void;
  /** Add a point at the given coordinates */
  addPoint: (latitude: number, longitude: number) => void;
  /** Remove the last drawn point (undo last) */
  removeLastPoint: () => void;
  /** Finish drawing: auto-appends end coordinate, validates, and dispatches operation */
  finishDrawing: () => string | null;
  /** Cancel drawing without submitting */
  cancelDrawing: () => void;
  /** Current total distance of drawn points in meters */
  currentDistance: number;
  /** Current number of drawn points */
  pointCount: number;
  /** Drawn points array */
  drawnPoints: Array<{ latitude: number; longitude: number; elevation?: number }>;
}

export function useDrawReplacementSection({
  state,
  dispatch,
  dispatchOperation,
}: UseDrawReplacementSectionOptions): UseDrawReplacementSectionResult {
  const drawing = state.drawing;
  const isDrawing = drawing?.isActive ?? false;
  const drawnPoints = drawing?.points ?? [];
  const pointCount = drawnPoints.length;

  const currentDistance = useMemo(() => {
    if (drawnPoints.length < 2) return 0;
    return polylineDistance(drawnPoints);
  }, [drawnPoints]);

  const startDrawing = useCallback(
    (selection: SectionSelection) => {
      const geometry = state.optimisticGeometry;
      if (!geometry) return;

      const segment = geometry[selection.segmentIndex];
      if (!segment) return;

      const startPoint = segment[selection.startIndex];
      if (!startPoint) return;

      dispatch({
        type: "DRAWING_START",
        segmentIndex: selection.segmentIndex,
        startIndex: selection.startIndex,
        endIndex: selection.endIndex,
        firstPoint: {
          latitude: startPoint.latitude,
          longitude: startPoint.longitude,
          ...(startPoint.elevation != null ? { elevation: startPoint.elevation } : {}),
        },
      });
    },
    [state.optimisticGeometry, dispatch],
  );

  const addPoint = useCallback(
    (latitude: number, longitude: number) => {
      if (!drawing || !drawing.isActive) return;
      // Client-side check: don't exceed max points (minus 1 for the end point that will be appended)
      if (drawing.points.length >= MAX_REPLACEMENT_POINTS - 1) return;
      dispatch({
        type: "DRAWING_ADD_POINT",
        point: { latitude, longitude },
      });
    },
    [drawing, dispatch],
  );

  const removeLastPoint = useCallback(() => {
    dispatch({ type: "DRAWING_REMOVE_LAST_POINT" });
  }, [dispatch]);

  const finishDrawing = useCallback((): string | null => {
    if (!drawing || !drawing.isActive) return "No active drawing to finish.";
    if (drawing.points.length < 2) return "At least 2 points are required to finish drawing.";

    const geometry = state.optimisticGeometry;
    if (!geometry) return "No geometry available.";

    const segment = geometry[drawing.segmentIndex];
    if (!segment) return "Invalid segment.";

    const endPoint = segment[drawing.endIndex];
    if (!endPoint) return "Invalid end point.";

    // Auto-append the end coordinate for endpoint continuity
    const finalPoints = [
      ...drawing.points,
      {
        latitude: endPoint.latitude,
        longitude: endPoint.longitude,
        ...(endPoint.elevation != null ? { elevation: endPoint.elevation } : {}),
      },
    ];

    // Validate max points
    if (finalPoints.length > MAX_REPLACEMENT_POINTS) {
      return `Too many points in the replacement (maximum ${MAX_REPLACEMENT_POINTS}).`;
    }

    // Construct and dispatch the operation
    const operation: ReplaceSectionOperation = {
      type: "replaceSection",
      segmentIndex: drawing.segmentIndex,
      startIndex: drawing.startIndex,
      endIndex: drawing.endIndex,
      replacement: finalPoints,
    };

    void dispatchOperation(operation);

    // Atomically clear drawing state
    dispatch({ type: "DRAWING_FINISH" });

    return null;
  }, [drawing, state.optimisticGeometry, dispatch, dispatchOperation]);

  const cancelDrawing = useCallback(() => {
    dispatch({ type: "DRAWING_CANCEL" });
  }, [dispatch]);

  return {
    isDrawing,
    startDrawing,
    addPoint,
    removeLastPoint,
    finishDrawing,
    cancelDrawing,
    currentDistance,
    pointCount,
    drawnPoints,
  };
}
