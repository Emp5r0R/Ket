package com.ket.android

import org.json.JSONArray
import org.json.JSONObject

internal data class GeoPoint(val longitude: Double, val latitude: Double)

internal data class WorldPolygon(val rings: List<List<GeoPoint>>)

internal data class ProjectedMapPoint(val x: Float, val y: Float)

internal class WorldMapGeometry(val polygons: List<WorldPolygon>) {
    init {
        require(polygons.isNotEmpty()) { "World map contains no polygons" }
    }
}

internal object WorldMapTopologyParser {
    private const val MAX_SOURCE_CHARACTERS = 160 * 1024
    private const val MAX_ARCS = 2_000
    private const val MAX_POINTS = 50_000

    fun parse(source: String): WorldMapGeometry {
        require(source.length in 1..MAX_SOURCE_CHARACTERS) { "World map source has an invalid size" }
        return try {
            parseTopology(source)
        } catch (error: IllegalArgumentException) {
            throw error
        } catch (error: Exception) {
            throw IllegalArgumentException("World map source is malformed", error)
        }
    }

    private fun parseTopology(source: String): WorldMapGeometry {
        val topology = JSONObject(source)
        require(topology.getString("type") == "Topology") { "World map source is not TopoJSON" }
        val transform = topology.getJSONObject("transform")
        val scale = pair(transform.getJSONArray("scale"), "World map scale")
        val translate = pair(transform.getJSONArray("translate"), "World map translation")
        require(scale.first > 0.0 && scale.second > 0.0) { "World map scale is invalid" }
        val decodedArcs = decodeArcs(topology.getJSONArray("arcs"), scale, translate)
        val land = topology.getJSONObject("objects").getJSONObject("land")
        require(land.getString("type") == "GeometryCollection") {
            "World map land object must be a geometry collection"
        }
        val polygons = buildList {
            val geometries = land.getJSONArray("geometries")
            for (index in 0 until geometries.length()) {
                val geometry = geometries.getJSONObject(index)
                when (geometry.getString("type")) {
                    "Polygon" -> add(decodePolygon(geometry.getJSONArray("arcs"), decodedArcs))
                    "MultiPolygon" -> {
                        val encodedPolygons = geometry.getJSONArray("arcs")
                        for (polygonIndex in 0 until encodedPolygons.length()) {
                            add(decodePolygon(encodedPolygons.getJSONArray(polygonIndex), decodedArcs))
                        }
                    }
                    else -> throw IllegalArgumentException("World map contains unsupported geometry")
                }
            }
        }
        return WorldMapGeometry(polygons)
    }

    private fun decodeArcs(
        encoded: JSONArray,
        scale: Pair<Double, Double>,
        translate: Pair<Double, Double>,
    ): List<List<GeoPoint>> {
        require(encoded.length() in 1..MAX_ARCS) { "World map arc count is invalid" }
        var pointCount = 0
        return buildList(encoded.length()) {
            for (arcIndex in 0 until encoded.length()) {
                val arc = encoded.getJSONArray(arcIndex)
                var x = 0
                var y = 0
                add(
                    buildList(arc.length()) {
                        for (pointIndex in 0 until arc.length()) {
                            val delta = arc.getJSONArray(pointIndex)
                            require(delta.length() == 2) { "World map coordinate is invalid" }
                            x = Math.addExact(x, delta.getInt(0))
                            y = Math.addExact(y, delta.getInt(1))
                            add(
                                GeoPoint(
                                    longitude = x * scale.first + translate.first,
                                    latitude = y * scale.second + translate.second,
                                ).also(::validatePoint),
                            )
                            pointCount += 1
                            require(pointCount <= MAX_POINTS) { "World map contains too many points" }
                        }
                    }.also { require(it.size >= 2) { "World map arc is too short" } },
                )
            }
        }
    }

    private fun decodePolygon(encodedRings: JSONArray, arcs: List<List<GeoPoint>>): WorldPolygon {
        require(encodedRings.length() > 0) { "World map polygon has no rings" }
        val rings = buildList(encodedRings.length()) {
            for (ringIndex in 0 until encodedRings.length()) {
                val references = encodedRings.getJSONArray(ringIndex)
                val ring = buildList {
                    for (referenceIndex in 0 until references.length()) {
                        val reference = references.getInt(referenceIndex)
                        val arcIndex = if (reference >= 0) reference else reference.inv()
                        require(arcIndex in arcs.indices) { "World map arc reference is invalid" }
                        val points = if (reference >= 0) arcs[arcIndex] else arcs[arcIndex].asReversed()
                        points.forEachIndexed { pointIndex, point ->
                            if (isEmpty() || pointIndex > 0) add(point)
                        }
                    }
                }
                require(ring.size >= 3) { "World map ring is too short" }
                add(ring)
            }
        }
        return WorldPolygon(rings)
    }

    private fun pair(value: JSONArray, label: String): Pair<Double, Double> {
        require(value.length() == 2) { "$label is invalid" }
        val first = value.getDouble(0)
        val second = value.getDouble(1)
        require(first.isFinite() && second.isFinite()) { "$label is invalid" }
        return first to second
    }

    private fun validatePoint(point: GeoPoint) {
        require(
            point.longitude.isFinite() &&
                point.latitude.isFinite() &&
                point.longitude in -180.001..180.001 &&
                point.latitude in -90.001..90.001,
        ) { "World map coordinate is outside geographic bounds" }
    }
}

internal object WorldMapProjection {
    fun project(
        longitude: Double,
        latitude: Double,
        width: Float,
        height: Float,
        horizontalPadding: Float = 0f,
        verticalPadding: Float = 0f,
    ): ProjectedMapPoint {
        require(longitude.isFinite() && longitude in -180.0..180.0) { "Longitude is invalid" }
        require(latitude.isFinite() && latitude in -90.0..90.0) { "Latitude is invalid" }
        require(width > horizontalPadding * 2 && height > verticalPadding * 2) {
            "Map viewport is invalid"
        }
        val contentWidth = width - horizontalPadding * 2
        val contentHeight = height - verticalPadding * 2
        return ProjectedMapPoint(
            x = horizontalPadding + ((longitude + 180.0) / 360.0 * contentWidth).toFloat(),
            y = verticalPadding + ((90.0 - latitude) / 180.0 * contentHeight).toFloat(),
        )
    }
}
