/** Tool modes available in the route editor */
export type EditorTool =
  | "select"
  | "move"
  | "add"
  | "delete"
  | "split"
  | "join"
  | "draw-section";

/** A selected single point on the route */
export interface PointSelection {
  type: "point";
  segmentIndex: number;
  pointIndex: number;
}

/** A selected contiguous section of a segment */
export interface SectionSelection {
  type: "section";
  segmentIndex: number;
  startIndex: number;
  endIndex: number;
}

/** The current selection state */
export type Selection = PointSelection | SectionSelection | null;

/** MovePoint operation payload */
export interface MovePointOperation {
  type: "MovePoint";
  segmentIndex: number;
  pointIndex: number;
  newPosition: { lat: number; lng: number };
}

/** AddPoint operation payload */
export interface AddPointOperation {
  type: "AddPoint";
  segmentIndex: number;
  afterPointIndex: number;
  point: { lat: number; lng: number; elevation?: number };
}

/** DeletePoint operation payload */
export interface DeletePointOperation {
  type: "DeletePoint";
  segmentIndex: number;
  pointIndex: number;
}

/** DeleteSection operation payload */
export interface DeleteSectionOperation {
  type: "DeleteSection";
  segmentIndex: number;
  startIndex: number;
  endIndex: number;
}

/** ReplaceSection operation payload */
export interface ReplaceSectionOperation {
  type: "ReplaceSection";
  segmentIndex: number;
  startIndex: number;
  endIndex: number;
  replacement: Array<{ lat: number; lng: number; elevation?: number }>;
}

/** SplitSegment operation payload */
export interface SplitSegmentOperation {
  type: "SplitSegment";
  segmentIndex: number;
  atPointIndex: number;
}

/** JoinSegments operation payload */
export interface JoinSegmentsOperation {
  type: "JoinSegments";
  firstSegmentIndex: number;
  secondSegmentIndex: number;
}

/** Union type for all route operations */
export type RouteOperation =
  | MovePointOperation
  | AddPointOperation
  | DeletePointOperation
  | DeleteSectionOperation
  | ReplaceSectionOperation
  | SplitSegmentOperation
  | JoinSegmentsOperation;

/** Editor state managed by the reducer */
export interface EditorState {
  currentTool: EditorTool;
  selection: Selection;
  draftId: string | null;
  revision: number;
  optimisticGeometry: number[][][] | null;
  canUndo: boolean;
  canRedo: boolean;
  conflictError: string | null;
  isOperationPending: boolean;
}

/** Actions dispatched to the editor state reducer */
export type EditorAction =
  | { type: "SET_TOOL"; tool: EditorTool }
  | { type: "SET_SELECTION"; selection: Selection }
  | { type: "CLEAR_SELECTION" }
  | { type: "SET_DRAFT"; draftId: string; revision: number; geometry: number[][][] }
  | { type: "OPERATION_START" }
  | { type: "OPERATION_SUCCESS"; revision: number; geometry: number[][][] }
  | { type: "OPERATION_FAILURE"; error: string }
  | { type: "SET_CONFLICT"; message: string }
  | { type: "CLEAR_CONFLICT" }
  | { type: "SET_OPTIMISTIC_GEOMETRY"; geometry: number[][][] }
  | { type: "SET_CAN_UNDO_REDO"; canUndo: boolean; canRedo: boolean };

/** Pending operation stored in IndexedDB for autosave/recovery */
export interface PendingOperation {
  id: string;
  draftId: string;
  operation: RouteOperation;
  expectedRevision: number;
  timestamp: number;
  confirmed: boolean;
}
