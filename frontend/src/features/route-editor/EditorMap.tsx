import { useEffect, useRef, useCallback, useState } from "react";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import type { Selection, EditorTool } from "./types";
import type { RoutePointDto } from "@/api/client";
import { computeSelection } from "./SelectionModel";

interface EditorMapProps {
  geometry: RoutePointDto[][] | null;
  baseGeometry: RoutePointDto[][] | null;
  selection: Selection;
  currentTool: EditorTool;
  onSelectionChange: (selection: Selection) => void;
  onMovePoint: (
    segmentIndex: number,
    pointIndex: number,
    newLng: number,
    newLat: number,
  ) => void;
  onAddPoint: (
    segmentIndex: number,
    afterPointIndex: number,
    lng: number,
    lat: number,
  ) => void;
}

/** Convert domain geometry to GeoJSON coordinate arrays for MapLibre rendering */
function geometryToCoords(geometry: RoutePointDto[][]): number[][][] {
  return geometry.map((segment) =>
    segment.map((pt) => [pt.longitude, pt.latitude, ...(pt.elevation != null ? [pt.elevation] : [])]),
  );
}

export function EditorMap({
  geometry,
  baseGeometry,
  selection,
  currentTool,
  onSelectionChange,
  onMovePoint,
  onAddPoint,
}: EditorMapProps) {
  const mapContainerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<maplibregl.Map | null>(null);
  const dragPointRef = useRef<{
    segmentIndex: number;
    pointIndex: number;
  } | null>(null);
  const isDraggingRef = useRef(false);
  const [tileError, setTileError] = useState(false);

  // Store latest props in refs for event handlers
  const selectionRef = useRef(selection);
  selectionRef.current = selection;
  const currentToolRef = useRef(currentTool);
  currentToolRef.current = currentTool;
  const onSelectionChangeRef = useRef(onSelectionChange);
  onSelectionChangeRef.current = onSelectionChange;
  const onMovePointRef = useRef(onMovePoint);
  onMovePointRef.current = onMovePoint;
  const onAddPointRef = useRef(onAddPoint);
  onAddPointRef.current = onAddPoint;

  // Initialize map
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

    // Tile error detection
    map.on("error", (e) => {
      if (
        e.error &&
        (e.error.message?.includes("tile") ||
          e.error.message?.includes("Tile") ||
          e.error.message?.includes("source"))
      ) {
        setTileError(true);
      }
    });

    map.on("load", () => {
      // Base route source and layer (rendered BELOW draft route)
      map.addSource("base-route", {
        type: "geojson",
        data: { type: "FeatureCollection", features: [] },
      });

      map.addLayer({
        id: "base-route-line",
        type: "line",
        source: "base-route",
        layout: { "line-join": "round", "line-cap": "butt" },
        paint: {
          "line-color": "#9ca3af",
          "line-width": 3,
          "line-opacity": 0.6,
          "line-dasharray": [4, 4],
        },
      });

      // Route line source and layer (draft/corrected route)
      map.addSource("route", {
        type: "geojson",
        data: createRouteGeoJSON([]),
      });

      map.addLayer({
        id: "route-line",
        type: "line",
        source: "route",
        layout: { "line-join": "round", "line-cap": "round" },
        paint: {
          "line-color": "#3b82f6",
          "line-width": 5,
          "line-opacity": 0.8,
        },
      });

      // Selection highlight layer
      map.addSource("selection-highlight", {
        type: "geojson",
        data: createSelectionGeoJSON([], null),
      });

      map.addLayer({
        id: "selection-highlight-layer",
        type: "line",
        source: "selection-highlight",
        layout: { "line-join": "round", "line-cap": "round" },
        paint: {
          "line-color": "#ef4444",
          "line-width": 7,
          "line-opacity": 0.8,
        },
      });

      // Route points source and layers
      map.addSource("route-points", {
        type: "geojson",
        data: createPointsGeoJSON([], null),
      });

      // Transparent hit-area circle layer for touch accuracy
      map.addLayer({
        id: "route-points-hitarea",
        type: "circle",
        source: "route-points",
        paint: {
          "circle-radius": [
            "interpolate",
            ["linear"],
            ["zoom"],
            10, 12,
            16, 22,
          ],
          "circle-color": "transparent",
          "circle-stroke-width": 0,
        },
      });

      // Visible points layer on top
      map.addLayer({
        id: "route-points-layer",
        type: "circle",
        source: "route-points",
        paint: {
          "circle-radius": [
            "case",
            ["==", ["get", "selected"], true],
            10,
            7,
          ],
          "circle-color": [
            "case",
            ["==", ["get", "selected"], true],
            "#ef4444",
            "#3b82f6",
          ],
          "circle-stroke-width": 2,
          "circle-stroke-color": "#ffffff",
        },
      });

      // Click handler for point selection (on hitarea for better touch targets)
      map.on("click", "route-points-hitarea", (e) => {
        if (isDraggingRef.current) return;
        const feature = e.features?.[0];
        if (!feature || !feature.properties) return;

        const segmentIndex = feature.properties["segmentIndex"] as number;
        const pointIndex = feature.properties["pointIndex"] as number;

        const newSelection = computeSelection(
          selectionRef.current,
          segmentIndex,
          pointIndex,
          e.originalEvent.shiftKey,
        );
        onSelectionChangeRef.current(newSelection);
      });

      // Click on route line to add point
      map.on("click", "route-line", (e) => {
        if (isDraggingRef.current) return;
        if (currentToolRef.current !== "add") return;

        const feature = e.features?.[0];
        if (!feature || !feature.properties) return;

        const segmentIndex = feature.properties["segmentIndex"] as number;
        const afterPointIndex = feature.properties[
          "afterPointIndex"
        ] as number;
        const lngLat = e.lngLat;
        onAddPointRef.current(
          segmentIndex,
          afterPointIndex,
          lngLat.lng,
          lngLat.lat,
        );
      });

      // Drag handling for move tool
      map.on("mousedown", "route-points-hitarea", (e) => {
        if (currentToolRef.current !== "move") return;
        const feature = e.features?.[0];
        if (!feature || !feature.properties) return;

        e.preventDefault();
        dragPointRef.current = {
          segmentIndex: feature.properties["segmentIndex"] as number,
          pointIndex: feature.properties["pointIndex"] as number,
        };
        isDraggingRef.current = true;
        map.getCanvas().style.cursor = "grabbing";
      });

      map.on("mousemove", (e) => {
        if (!isDraggingRef.current || !dragPointRef.current) return;
        // Visual feedback during drag (cursor already set)
        void e;
      });

      map.on("mouseup", (e) => {
        if (!isDraggingRef.current || !dragPointRef.current) return;
        const { segmentIndex, pointIndex } = dragPointRef.current;
        const lngLat = e.lngLat;
        onMovePointRef.current(
          segmentIndex,
          pointIndex,
          lngLat.lng,
          lngLat.lat,
        );
        dragPointRef.current = null;
        isDraggingRef.current = false;
        map.getCanvas().style.cursor = "";
      });

      // Cursor management
      map.on("mouseenter", "route-points-hitarea", () => {
        if (currentToolRef.current === "move") {
          map.getCanvas().style.cursor = "grab";
        } else {
          map.getCanvas().style.cursor = "pointer";
        }
      });

      map.on("mouseleave", "route-points-hitarea", () => {
        if (!isDraggingRef.current) {
          map.getCanvas().style.cursor = "";
        }
      });
    });

    mapRef.current = map;

    return () => {
      map.remove();
      mapRef.current = null;
    };
  }, []);

  // Update geometry on map when it changes
  const updateMap = useCallback(() => {
    const map = mapRef.current;
    if (!map || !map.isStyleLoaded()) return;

    const coords = geometry ? geometryToCoords(geometry) : [];

    // Update base route
    const baseCoords = baseGeometry ? geometryToCoords(baseGeometry) : [];
    const baseRouteSource = map.getSource("base-route") as maplibregl.GeoJSONSource | undefined;
    if (baseRouteSource) {
      baseRouteSource.setData(createBaseRouteGeoJSON(baseCoords));
    }

    // Update draft route
    const routeSource = map.getSource("route") as maplibregl.GeoJSONSource | undefined;
    if (routeSource) {
      routeSource.setData(createRouteGeoJSON(coords));
    }

    const pointsSource = map.getSource("route-points") as maplibregl.GeoJSONSource | undefined;
    if (pointsSource) {
      pointsSource.setData(createPointsGeoJSON(coords, selection));
    }

    const selectionSource = map.getSource("selection-highlight") as maplibregl.GeoJSONSource | undefined;
    if (selectionSource) {
      selectionSource.setData(createSelectionGeoJSON(coords, selection));
    }
  }, [geometry, baseGeometry, selection]);

  useEffect(() => {
    updateMap();
  }, [updateMap]);

  // Fit bounds when geometry first loads
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !geometry || geometry.length === 0) return;

    const bounds = new maplibregl.LngLatBounds();
    let hasPoints = false;
    for (const segment of geometry) {
      for (const pt of segment) {
        bounds.extend([pt.longitude, pt.latitude]);
        hasPoints = true;
      }
    }
    if (hasPoints) {
      map.fitBounds(bounds, { padding: 50 });
    }
    // Only run on initial geometry load
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [geometry !== null]);

  return (
    <div className="relative h-full w-full">
      <div
        ref={mapContainerRef}
        className="h-full w-full"
        role="application"
        aria-label="Route editor map"
        aria-roledescription="Interactive map for editing route geometry"
        tabIndex={0}
      />
      {tileError && (
        <div
          className="absolute inset-x-0 top-0 flex items-center justify-center gap-3 bg-black/60 px-4 py-3"
          role="alert"
          aria-live="polite"
        >
          <p className="text-sm font-medium text-white">
            Map tiles failed to load. Check your connection.
          </p>
          <button
            type="button"
            className="ml-2 rounded bg-white/20 px-2 py-1 text-xs font-medium text-white hover:bg-white/30 focus:outline-none focus:ring-2 focus:ring-white"
            onClick={() => setTileError(false)}
            aria-label="Dismiss tile error"
          >
            Dismiss
          </button>
        </div>
      )}
    </div>
  );
}

/** Create a GeoJSON FeatureCollection for the base route lines */
function createBaseRouteGeoJSON(
  geometry: number[][][],
): GeoJSON.FeatureCollection {
  const features: GeoJSON.Feature[] = geometry.map((segment) => ({
    type: "Feature",
    properties: {},
    geometry: {
      type: "LineString",
      coordinates: segment,
    },
  }));

  return { type: "FeatureCollection", features };
}

/** Create a GeoJSON FeatureCollection for the route lines with per-segment features */
function createRouteGeoJSON(
  geometry: number[][][],
): GeoJSON.FeatureCollection {
  const features: GeoJSON.Feature[] = [];

  for (let segIdx = 0; segIdx < geometry.length; segIdx++) {
    const segment = geometry[segIdx];
    if (!segment || segment.length < 2) continue;

    // Create per-segment line features with segmentIndex property
    // Also create per-edge features so click events can identify where to add points
    for (let ptIdx = 0; ptIdx < segment.length - 1; ptIdx++) {
      const start = segment[ptIdx];
      const end = segment[ptIdx + 1];
      if (!start || !end) continue;

      features.push({
        type: "Feature",
        properties: {
          segmentIndex: segIdx,
          afterPointIndex: ptIdx,
        },
        geometry: {
          type: "LineString",
          coordinates: [start, end],
        },
      });
    }
  }

  return { type: "FeatureCollection", features };
}

/** Create a GeoJSON FeatureCollection for the route points */
function createPointsGeoJSON(
  geometry: number[][][],
  selection: Selection,
): GeoJSON.FeatureCollection {
  const features: GeoJSON.Feature[] = [];

  for (let segIdx = 0; segIdx < geometry.length; segIdx++) {
    const segment = geometry[segIdx];
    if (!segment) continue;
    for (let ptIdx = 0; ptIdx < segment.length; ptIdx++) {
      const coord = segment[ptIdx];
      if (!coord) continue;
      const isSelected = isPointSelected(segIdx, ptIdx, selection);
      features.push({
        type: "Feature",
        properties: {
          segmentIndex: segIdx,
          pointIndex: ptIdx,
          selected: isSelected,
        },
        geometry: {
          type: "Point",
          coordinates: coord,
        },
      });
    }
  }

  return { type: "FeatureCollection", features };
}

/** Create a GeoJSON FeatureCollection for the selection highlight */
function createSelectionGeoJSON(
  geometry: number[][][],
  selection: Selection,
): GeoJSON.FeatureCollection {
  if (!selection || selection.type !== "section") {
    return { type: "FeatureCollection", features: [] };
  }

  const segment = geometry[selection.segmentIndex];
  if (!segment) {
    return { type: "FeatureCollection", features: [] };
  }

  const coords = segment.slice(selection.startIndex, selection.endIndex + 1);
  if (coords.length < 2) {
    return { type: "FeatureCollection", features: [] };
  }

  return {
    type: "FeatureCollection",
    features: [
      {
        type: "Feature",
        properties: {},
        geometry: {
          type: "LineString",
          coordinates: coords,
        },
      },
    ],
  };
}

function isPointSelected(
  segmentIndex: number,
  pointIndex: number,
  selection: Selection,
): boolean {
  if (!selection) return false;
  if (selection.type === "point") {
    return (
      selection.segmentIndex === segmentIndex &&
      selection.pointIndex === pointIndex
    );
  }
  if (selection.type === "section") {
    return (
      selection.segmentIndex === segmentIndex &&
      pointIndex >= selection.startIndex &&
      pointIndex <= selection.endIndex
    );
  }
  return false;
}
