import { useEffect, useRef } from "react";
import maplibregl from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
import type { RecordedRoute } from "@/api/client";

interface RouteMapProps {
  route: RecordedRoute;
}

export function RouteMap({ route }: RouteMapProps) {
  const mapContainerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<maplibregl.Map | null>(null);

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

    map.on("load", () => {
      // Build GeoJSON from route features
      const geojsonData = {
        type: "FeatureCollection" as const,
        features: route.features.map((f) => ({
          type: "Feature" as const,
          geometry: {
            type: f.geometry.type as "LineString",
            coordinates: f.geometry.coordinates,
          },
          properties: f.properties,
        })),
      };

      // Add route line
      map.addSource("route", {
        type: "geojson",
        data: geojsonData,
      });

      map.addLayer({
        id: "route-line",
        type: "line",
        source: "route",
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

      // Extract all coordinates in order across segments
      const allCoords = getAllCoordinates(route);

      if (allCoords.length > 0) {
        const startCoord = allCoords[0]!;
        const endCoord = allCoords[allCoords.length - 1]!;

        // Add start marker (green)
        const startEl = createMarkerElement("start");
        new maplibregl.Marker({ element: startEl })
          .setLngLat(startCoord)
          .addTo(map);

        // Add end marker (red)
        const endEl = createMarkerElement("end");
        new maplibregl.Marker({ element: endEl })
          .setLngLat(endCoord)
          .addTo(map);

        // Add kilometer markers
        const kmPositions = computeKilometerPositions(allCoords);
        for (const { km, position } of kmPositions) {
          const kmEl = createKmMarkerElement(km);
          new maplibregl.Marker({ element: kmEl, anchor: "center" })
            .setLngLat(position)
            .addTo(map);
        }
      }

      // Fit bounds to route
      if (route.bbox.length === 4) {
        const [west, south, east, north] = route.bbox;
        map.fitBounds(
          [[west!, south!], [east!, north!]],
          { padding: 50 },
        );
      }
    });

    mapRef.current = map;

    return () => {
      map.remove();
      mapRef.current = null;
    };
  }, [route]);

  return (
    <div
      ref={mapContainerRef}
      className="h-64 w-full rounded-lg border border-gray-200 sm:h-96"
      role="img"
      aria-label="Route map"
      tabIndex={0}
    />
  );
}

/** Extract all [lng, lat] coordinates in order from the route. */
function getAllCoordinates(route: RecordedRoute): [number, number][] {
  const coords: [number, number][] = [];
  for (const feature of route.features) {
    for (const coord of feature.geometry.coordinates) {
      coords.push(coord as [number, number]);
    }
  }
  return coords;
}

/** Create a start (green) or end (red) marker DOM element. */
function createMarkerElement(type: "start" | "end"): HTMLElement {
  const el = document.createElement("div");
  el.className = "route-marker";
  const color = type === "start" ? "#22c55e" : "#ef4444";
  const label = type === "start" ? "Start" : "End";
  el.style.cssText = `
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background-color: ${color};
    border: 3px solid white;
    box-shadow: 0 1px 4px rgba(0,0,0,0.4);
    cursor: default;
  `;
  el.setAttribute("aria-label", label);
  el.setAttribute("role", "img");
  return el;
}

/** Create a kilometer marker DOM element with the km number. */
function createKmMarkerElement(km: number): HTMLElement {
  const el = document.createElement("div");
  el.className = "km-marker";
  el.style.cssText = `
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background-color: white;
    border: 2px solid #3b82f6;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    font-weight: 600;
    color: #3b82f6;
    box-shadow: 0 1px 3px rgba(0,0,0,0.3);
    cursor: default;
  `;
  el.textContent = String(km);
  el.setAttribute("aria-label", `${km} km`);
  el.setAttribute("role", "img");
  return el;
}

/** Compute the haversine distance in meters between two [lng, lat] points. */
function haversineMeters(a: [number, number], b: [number, number]): number {
  const toRad = (deg: number) => (deg * Math.PI) / 180;
  const R = 6371000;
  const dLat = toRad(b[1] - a[1]);
  const dLng = toRad(b[0] - a[0]);
  const sinLat = Math.sin(dLat / 2);
  const sinLng = Math.sin(dLng / 2);
  const h =
    sinLat * sinLat +
    Math.cos(toRad(a[1])) * Math.cos(toRad(b[1])) * sinLng * sinLng;
  return 2 * R * Math.asin(Math.sqrt(h));
}

/** Find the [lng, lat] positions for each whole kilometer along the route. */
function computeKilometerPositions(
  coords: [number, number][],
): { km: number; position: [number, number] }[] {
  const results: { km: number; position: [number, number] }[] = [];
  if (coords.length < 2) return results;

  let cumulativeDistance = 0;
  let nextKm = 1000; // meters

  for (let i = 1; i < coords.length; i++) {
    const prev = coords[i - 1]!;
    const curr = coords[i]!;
    const segmentDist = haversineMeters(prev, curr);

    while (cumulativeDistance + segmentDist >= nextKm) {
      // Interpolate position at the km boundary
      const remaining = nextKm - cumulativeDistance;
      const fraction = remaining / segmentDist;
      const lng = prev[0] + (curr[0] - prev[0]) * fraction;
      const lat = prev[1] + (curr[1] - prev[1]) * fraction;
      results.push({ km: nextKm / 1000, position: [lng, lat] });
      nextKm += 1000;
    }

    cumulativeDistance += segmentDist;
  }

  return results;
}
