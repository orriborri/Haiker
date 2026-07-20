/**
 * Shared formatting utilities used across multiple domains.
 */

/**
 * Format a date string to a localized short date (e.g. "Jan 15, 2024").
 */
export function formatDate(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/**
 * Format a date string to a localized date+time (e.g. "Jan 15, 2024, 10:30 AM").
 */
export function formatDateTime(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * Format a distance in meters to a human-readable km string (e.g. "5.2 km").
 */
export function formatDistanceKm(meters: number): string {
  return `${(meters / 1000).toFixed(1)} km`;
}
