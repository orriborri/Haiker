import { useEffect } from "react";

/**
 * Updates the document title for accessibility and browser tab context.
 * Appends " - Haiker" suffix unless the title is empty.
 */
export function useDocumentTitle(title: string): void {
  useEffect(() => {
    const fullTitle = title ? `${title} - Haiker` : "Haiker";
    document.title = fullTitle;
  }, [title]);
}
