import type { RouteVersionSummary } from "@/api/client";

interface VersionSelectorProps {
  versions: RouteVersionSummary[];
  selectedVersionId: string;
  onVersionChange: (versionId: string) => void;
}

export function VersionSelector({
  versions,
  selectedVersionId,
  onVersionChange,
}: VersionSelectorProps) {
  return (
    <div className="flex items-center gap-2">
      <label
        htmlFor="version-selector"
        className="text-sm font-medium text-gray-700"
      >
        Compare with:
      </label>
      <select
        id="version-selector"
        value={selectedVersionId}
        onChange={(e) => onVersionChange(e.target.value)}
        className="rounded-md border border-gray-300 bg-white px-3 py-1.5 text-sm text-gray-900 shadow-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
      >
        {versions.map((version) => (
          <option key={version.id} value={version.id}>
            v{version.versionNumber}
            {version.editSummary ? ` - ${version.editSummary}` : ""}
          </option>
        ))}
      </select>
    </div>
  );
}
