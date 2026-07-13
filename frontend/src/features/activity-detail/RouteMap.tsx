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
      map.addSource("route", {
        type: "geojson",
        data: route as unknown as GeoJSON.FeatureCollection,
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

      // Fit bounds to route
      const coordinates = extractCoordinates(route);
      if (coordinates.length > 0) {
        const bounds = new maplibregl.LngLatBounds();
        for (const coord of coordinates) {
          bounds.extend(coord as [number, number]);
        }
        map.fitBounds(bounds, { padding: 50 });
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

function extractCoordinates(route: RecordedRoute): [number, number][] {
  const coords: [number, number][] = [];
  for (const feature of route.features) {
    const geom = feature.geometry;
    if (geom.type === "LineString") {
      for (const coord of geom.coordinates) {
        coords.push(coord as unknown as [number, number]);
      }
    } else if (geom.type === "MultiLineString") {
      for (const line of geom.coordinates as unknown as number[][][]) {
        for (const coord of line) {
          coords.push(coord as unknown as [number, number]);
        }
      }
    }
  }
  return coords;
}
