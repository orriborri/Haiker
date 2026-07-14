import { useState, useEffect, useCallback } from "react";

interface UseOnlineStatusResult {
  isOnline: boolean;
}

/**
 * Tracks browser network connectivity via navigator.onLine and
 * window online/offline events.
 */
export function useOnlineStatus(): UseOnlineStatusResult {
  const [isOnline, setIsOnline] = useState(() => navigator.onLine);

  const handleOnline = useCallback(() => {
    setIsOnline(true);
  }, []);

  const handleOffline = useCallback(() => {
    setIsOnline(false);
  }, []);

  useEffect(() => {
    window.addEventListener("online", handleOnline);
    window.addEventListener("offline", handleOffline);

    return () => {
      window.removeEventListener("online", handleOnline);
      window.removeEventListener("offline", handleOffline);
    };
  }, [handleOnline, handleOffline]);

  return { isOnline };
}
