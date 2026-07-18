import { useMemo } from "react";
import { geoNaturalEarth1, geoPath } from "d3-geo";
import isoCountries from "i18n-iso-countries";
import { feature } from "topojson-client";
import type { FeatureCollection, Geometry } from "geojson";
import type { GeometryCollection, Topology } from "topojson-specification";
import atlasSource from "world-atlas/countries-110m.json";
import type { NodeLocation } from "../types";

interface WorldMapProps {
  location: NodeLocation | null;
  connected: boolean;
}

type WorldTopology = Topology<{ countries: GeometryCollection }>;

const atlas = atlasSource as unknown as WorldTopology;
const countryCollection = feature(
  atlas,
  atlas.objects.countries,
) as unknown as FeatureCollection<Geometry>;

export function WorldMap({ location, connected }: WorldMapProps) {
  const { paths, marker } = useMemo(() => {
    const projection = geoNaturalEarth1().fitExtent(
      [
        [24, 20],
        [876, 412],
      ],
      countryCollection,
    );
    const renderPath = geoPath(projection);
    const numericCode = location
      ? isoCountries.alpha2ToNumeric(location.country_code.toUpperCase())
      : undefined;
    const rendered = countryCollection.features.map((country, index) => ({
      id: country.id == null ? `country-${index}` : String(country.id),
      path: renderPath(country) ?? "",
      active: Boolean(numericCode) && String(country.id).padStart(3, "0") === numericCode,
    }));
    return {
      paths: rendered,
      marker: location ? projection([location.longitude, location.latitude]) : null,
    };
  }, [location]);

  return (
    <div className={`world-map ${connected ? "is-connected" : ""}`}>
      <svg
        viewBox="0 0 900 432"
        role="img"
        aria-label={
          location
            ? `Server location: ${location.city ?? location.country_name}, ${location.country_name}`
            : "Available server regions"
        }
      >
        <g className="map-countries">
          {paths.map((country) => (
            <path
              key={country.id}
              d={country.path}
              className={country.active ? "map-country is-active" : "map-country"}
            />
          ))}
        </g>
        {marker && location ? (
          <g className="map-marker" transform={`translate(${marker[0]} ${marker[1]})`}>
            <circle className="map-marker-ring" r="18" />
            <circle className="map-marker-dot" r="5" />
            <text x="14" y="-13">
              {location.city ?? location.country_name}
            </text>
          </g>
        ) : null}
      </svg>
    </div>
  );
}
