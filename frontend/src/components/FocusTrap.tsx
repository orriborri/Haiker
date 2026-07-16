import { useEffect, useRef, type ReactNode } from "react";

interface FocusTrapProps {
  children: ReactNode;
  onEscape?: () => void;
}

/**
 * Traps focus within its container for modal dialogs.
 * Restores focus to the previously focused element on unmount.
 * Calls onEscape when the Escape key is pressed.
 */
export function FocusTrap({ children, onEscape }: FocusTrapProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    previousFocusRef.current = document.activeElement as HTMLElement | null;

    // Focus the first focusable element in the container
    const container = containerRef.current;
    if (container) {
      const focusable = getFocusableElements(container);
      if (focusable.length > 0) {
        focusable[0]!.focus();
      } else {
        container.focus();
      }
    }

    return () => {
      // Restore focus on unmount
      previousFocusRef.current?.focus();
    };
  }, []);

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape" && onEscape) {
        e.preventDefault();
        onEscape();
        return;
      }

      if (e.key === "Tab") {
        const container = containerRef.current;
        if (!container) return;

        const focusable = getFocusableElements(container);
        if (focusable.length === 0) {
          e.preventDefault();
          return;
        }

        const firstElement = focusable[0]!;
        const lastElement = focusable[focusable.length - 1]!;

        if (e.shiftKey) {
          if (document.activeElement === firstElement) {
            e.preventDefault();
            lastElement.focus();
          }
        } else {
          if (document.activeElement === lastElement) {
            e.preventDefault();
            firstElement.focus();
          }
        }
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onEscape]);

  return (
    <div ref={containerRef} tabIndex={-1} style={{ outline: "none" }}>
      {children}
    </div>
  );
}

function getFocusableElements(container: HTMLElement): HTMLElement[] {
  const selector =
    'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';
  return Array.from(container.querySelectorAll<HTMLElement>(selector));
}
