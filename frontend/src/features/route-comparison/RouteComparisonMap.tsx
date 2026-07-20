import { useCallback, useEffect, useRef, useState } from "react";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import type { RouteComparisonResponse } from "@/api/client";

interface RouteComparisonMapProps {
  comparison: RouteComparisonResponse;
}

export function RouteComparisonMap({ comparison }: RouteComparisonMapProps) {
  const mapContainerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<maplibregl.Map | null>(null);
  const markersRef = useRef<maplibregl.Marker[]>([]);
  const mapLoadedRef = useRef(false);
  const [tileError, setTileError] = useState(false);

  // Effect 1: Create the map instance once
  useEffect(() => {
    const container = mapContainerRef.current;
    if (!container) return;

    const map = new maplibregl.Map({
      container,
      style: {
        version: 8,
        sources: {
          osm: {
            type: "raster",
            tiles: ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"],
            tileSize: 256,
            attribution: "&copy; OpenStreetMap contributors",
          },
        },
        layers: [
          {
            id: "osm-tiles",
            type: "raster",
            source: "osm",
            minzoom: 0,
            maxzoom: 19,
          },
        ],
      },
      center: [0, 0],
      zoom: 2,
      attributionControl: {},
      keyboard: true,
    });

    map.addControl(new maplibregl.NavigationControl(), "top-right");

    // Listen for tile errors
    map.on("error", (e) => {
      if (e.error && e.error.message && /tile/i.test(e.error.message)) {
        setTileError(true);
      }
    });

    map.on("load", () => {
      // Add empty sources and layers on first load so they can be updated later
      map.addSource("recorded-route", {
        type: "geojson",
        data: { type: "FeatureCollection", features: [] },
      });

      map.addLayer({
        id: "recorded-route-line",
        type: "line",
        source: "recorded-route",
        layout: {
          "line-join": "round",
          "line-cap": "round",
        },
        paint: {
          "line-color": "#3b82f6",
          "line-width": 4,
          "line-opacity": 0.8,
        },
      });

      map.addSource("corrected-route", {
        type: "geojson",
        data: { type: "FeatureCollection", features: [] },
      });

      map.addLayer({
        id: "corrected-route-line",
        type: "line",
        source: "corrected-route",
        layout: {
          "line-join": "round",
          "line-cap": "butt",
        },
        paint: {
          "line-color": "#f97316",
          "line-width": 4,
          "line-opacity": 0.8,
          "line-dasharray": [8, 4],
        },
      });

      mapLoadedRef.current = true;
    });

    mapRef.current = map;

    return () => {
      mapLoadedRef.current = false;
      map.remove();
      mapRef.current = null;
    };
  }, []);

  const updateMapData = useCallback(() => {
    const map = mapRef.current;
    if (!map || !mapLoadedRef.current) return;

    // Update recorded route source data
    const recordedGeoJson = {
      type: "FeatureCollection" as const,
      features: comparison.recorded.geometry.features.map((f) => ({
        type: "Feature" as const,
        geometry: {
          type: f.geometry.type as "LineString",
          coordinates: f.geometry.coordinates,
        },
        properties: f.properties,
      })),
    };

    const recordedSource = map.getSource(
      "recorded-route",
    ) as maplibregl.GeoJSONSource | undefined;
    if (recordedSource) {
      recordedSource.setData(recordedGeoJson);
    }

    // Update corrected route source data
    const correctedGeoJson = {
      type: "FeatureCollection" as const,
      features: comparison.corrected.geometry.features.map((f) => ({
        type: "Feature" as const,
        geometry: {
          type: f.geometry.type as "LineString",
          coordinates: f.geometry.coordinates,
        },
        properties: f.properties,
      })),
    };

    const correctedSource = map.getSource(
      "corrected-route",
    ) as maplibregl.GeoJSONSource | undefined;
    if (correctedSource) {
      correctedSource.setData(correctedGeoJson);
    }

    // Remove existing markers
    for (const marker of markersRef.current) {
      marker.remove();
    }
    markersRef.current = [];

    // Add start/end markers for recorded route (blue)
    const recordedCoords = getAllCoordinates(
      comparison.recorded.geometry.features,
    );
    if (recordedCoords.length > 0) {
      const startCoord = recordedCoords[0]!;
      const endCoord = recordedCoords[recordedCoords.length - 1]!;

      const startEl = createMarkerElement("recorded-start");
      const startMarker = new maplibregl.Marker({ element: startEl })
        .setLngLat(startCoord)
        .addTo(map);
      markersRef.current.push(startMarker);

      const endEl = createMarkerElement("recorded-end");
      const endMarker = new maplibregl.Marker({ element: endEl })
        .setLngLat(endCoord)
        .addTo(map);
      markersRef.current.push(endMarker);
    }

    // Add start/end markers for corrected route (orange)
    const correctedCoords = getAllCoordinates(
      comparison.corrected.geometry.features,
    );
    if (correctedCoords.length > 0) {
      const startCoord = correctedCoords[0]!;
      const endCoord = correctedCoords[correctedCoords.length - 1]!;

      const startEl = createMarkerElement("corrected-start");
      const startMarker = new maplibregl.Marker({ element: startEl })
        .setLngLat(startCoord)
        .addTo(map);
      markersRef.current.push(startMarker);

      const endEl = createMarkerElement("corrected-end");
      const endMarker = new maplibregl.Marker({ element: endEl })
        .setLngLat(endCoord)
        .addTo(map);
      markersRef.current.push(endMarker);
    }

    // Fit bounds to shared bbox
    if (comparison.sharedBbox.length === 4) {
      const [west, south, east, north] = comparison.sharedBbox;
      map.fitBounds(
        [
          [west!, south!],
          [east!, north!],
        ],
        { padding: 50 },
      );
    }
  }, [comparison]);

  // Effect 2: Update GeoJSON sources and markers when comparison data changes
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;

    if (mapLoadedRef.current) {
      // Map is already loaded, update immediately
      updateMapData();
    } else {
      // Map is still loading, wait for load event
      const onLoad = () => {
        updateMapData();
      };
      map.on("load", onLoad);
      return () => {
        map.off("load", onLoad);
      };
    }
  }, [updateMapData]);

  return (
    <div>
      <div
        ref={mapContainerRef}
        className="h-64 w-full rounded-lg border border-gray-200 sm:h-96"
        role="img"
        aria-label="Route comparison map showing recorded and corrected routes"
        tabIndex={0}
      />
      {tileError && (
        <div
          className="mt-2 rounded-md border border-yellow-300 bg-yellow-50 px-3 py-2 text-sm text-yellow-800"
          role="alert"
        >
          Map tiles failed to load. Route lines are still visible.
        </div>
      )}
    </div>
  );
}

interface GeoJsonFeature {
  geometry: { type: string; coordinates: number[][] };
  properties: unknown;
}

function getAllCoordinates(features: GeoJsonFeature[]): [number, number][] {
  const coords: [number, number][] = [];
  for (const feature of features) {
    for (const coord of feature.geometry.coordinates) {
      coords.push(coord as [number, number]);
    }
  }
  return coords;
}

type MarkerType =
  | "recorded-start"
  | "recorded-end"
  | "corrected-start"
  | "corrected-end";

function createMarkerElement(type: MarkerType): HTMLElement {
  const el = document.createElement("div");
  el.className = "route-comparison-marker";

  // Markers use the same colors as lines: blue for recorded, orange for corrected
  const colorMap: Record<MarkerType, string> = {
    "recorded-start": "#3b82f6",
    "recorded-end": "#3b82f6",
    "corrected-start": "#f97316",
    "corrected-end": "#f97316",
  };

  const labelMap: Record<MarkerType, string> = {
    "recorded-start": "Recorded route start",
    "recorded-end": "Recorded route end",
    "corrected-start": "Corrected route start",
    "corrected-end": "Corrected route end",
  };

  const isStart = type.endsWith("-start");
  const borderStyle = type.startsWith("corrected") ? "dashed" : "solid";

  el.style.cssText = `
    width: 14px;
    height: 14px;
    border-radius: ${isStart ? "50%" : "2px"};
    background-color: ${colorMap[type]};
    border: 2px ${borderStyle} white;
    box-shadow: 0 1px 4px rgba(0,0,0,0.4);
    cursor: default;
  `;
  el.setAttribute("aria-label", labelMap[type]);
  el.setAttribute("role", "img");
  return el;
}
