import type { Selection, PointSelection, SectionSelection } from "./types";

/**
 * Selection model for managing point and section selections
 * against the current route geometry.
 */
export class SelectionModel {
  private _selection: Selection = null;

  get selection(): Selection {
    return this._selection;
  }

  get selectedPoint(): PointSelection | null {
    return this._selection?.type === "point" ? this._selection : null;
  }

  get selectedSection(): SectionSelection | null {
    return this._selection?.type === "section" ? this._selection : null;
  }

  selectPoint(segmentIndex: number, pointIndex: number): void {
    this._selection = { type: "point", segmentIndex, pointIndex };
  }

  selectSection(
    segmentIndex: number,
    startIndex: number,
    endIndex: number,
  ): void {
    const actualStart = Math.min(startIndex, endIndex);
    const actualEnd = Math.max(startIndex, endIndex);
    this._selection = {
      type: "section",
      segmentIndex,
      startIndex: actualStart,
      endIndex: actualEnd,
    };
  }

  clear(): void {
    this._selection = null;
  }

  /**
   * Validate the current selection against geometry dimensions.
   * Returns true if the selection is still valid, false if it should be cleared.
   */
  validate(geometry: number[][][]): boolean {
    if (!this._selection) return true;

    if (this._selection.type === "point") {
      const segment = geometry[this._selection.segmentIndex];
      if (!segment) return false;
      if (this._selection.pointIndex >= segment.length) return false;
      return true;
    }

    if (this._selection.type === "section") {
      const segment = geometry[this._selection.segmentIndex];
      if (!segment) return false;
      if (this._selection.endIndex >= segment.length) return false;
      return true;
    }

    return false;
  }
}

/**
 * Create a new selection state from a point click.
 * If shift is held and there is an existing point selection on the same segment,
 * creates a section selection between them.
 */
export function computeSelection(
  current: Selection,
  segmentIndex: number,
  pointIndex: number,
  shiftKey: boolean,
): Selection {
  if (
    shiftKey &&
    current?.type === "point" &&
    current.segmentIndex === segmentIndex
  ) {
    const start = Math.min(current.pointIndex, pointIndex);
    const end = Math.max(current.pointIndex, pointIndex);
    if (start === end) {
      return { type: "point", segmentIndex, pointIndex };
    }
    return { type: "section", segmentIndex, startIndex: start, endIndex: end };
  }

  return { type: "point", segmentIndex, pointIndex };
}
