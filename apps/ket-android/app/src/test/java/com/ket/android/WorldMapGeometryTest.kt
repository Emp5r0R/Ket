package com.ket.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class WorldMapGeometryTest {
    @Test
    fun `bundled natural earth topology decodes into the expected land coverage`() {
        val source = requireNotNull(
            javaClass.classLoader?.getResourceAsStream("world_land_110m.json"),
        ).bufferedReader().use { it.readText() }

        val geometry = WorldMapTopologyParser.parse(source)
        val points = geometry.polygons.sumOf { polygon -> polygon.rings.sumOf { ring -> ring.size } }

        assertEquals(125, geometry.polygons.size)
        assertTrue(points > 5_000)
    }

    @Test
    fun `decodes delta encoded polygons and reversed topojson arcs`() {
        val geometry = WorldMapTopologyParser.parse(
            """{
                "type":"Topology",
                "objects":{"land":{"type":"GeometryCollection","geometries":[
                    {"type":"Polygon","arcs":[[0,-2]]}
                ]}},
                "arcs":[[[0,0],[10,0],[0,10]],[[0,0],[0,10],[10,0]]],
                "transform":{"scale":[1,1],"translate":[0,0]}
            }""".trimIndent(),
        )

        val ring = geometry.polygons.single().rings.single()

        assertEquals(5, ring.size)
        assertEquals(GeoPoint(0.0, 0.0), ring.first())
        assertEquals(GeoPoint(0.0, 0.0), ring.last())
        assertTrue(ring.contains(GeoPoint(10.0, 10.0)))
    }

    @Test
    fun `projects geographic extremes and a real node inside a stable viewport`() {
        val northWest = WorldMapProjection.project(-180.0, 90.0, 360f, 180f)
        val southEast = WorldMapProjection.project(180.0, -90.0, 360f, 180f)
        val frankfurt = WorldMapProjection.project(8.6821, 50.1109, 360f, 180f)

        assertEquals(0f, northWest.x, 0.001f)
        assertEquals(0f, northWest.y, 0.001f)
        assertEquals(360f, southEast.x, 0.001f)
        assertEquals(180f, southEast.y, 0.001f)
        assertTrue(frankfurt.x in 180f..200f)
        assertTrue(frankfurt.y in 35f..45f)
    }

    @Test
    fun `rejects malformed topology and invalid projection inputs`() {
        assertThrows(IllegalArgumentException::class.java) {
            WorldMapTopologyParser.parse("{}")
        }
        assertThrows(IllegalArgumentException::class.java) {
            WorldMapProjection.project(181.0, 0.0, 360f, 180f)
        }
        assertThrows(IllegalArgumentException::class.java) {
            WorldMapProjection.project(0.0, 0.0, 0f, 180f)
        }
    }
}
