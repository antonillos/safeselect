package com.safeselect;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;

import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

class MainTest {
    @Test
    void rejectsServerSideJavaScriptOperatorsAtAnyDepth() {
        for (String operator : List.of("$where", "$function", "$accumulator")) {
            Object nested = Map.of("$and", List.of(Map.of("nested", Map.of(operator, Map.of("body", "never execute")))));
            assertEquals(operator, Main.forbiddenMongoJavaScriptOperator(nested));
        }
    }

    @Test
    void allowsDeclarativeMql() {
        Object declarative = Map.of("$and", List.of(Map.of("active", true), Map.of("score", Map.of("$gte", 10))));
        assertNull(Main.forbiddenMongoJavaScriptOperator(declarative));
    }

    @Test
    void preservesOnlyKnownSearchIndexTypes() {
        assertEquals("search", Main.searchIndexType("search"));
        assertEquals("vectorSearch", Main.searchIndexType("vectorSearch"));
        assertEquals("autoEmbed", Main.searchIndexType("autoEmbed"));
        assertEquals("unknown", Main.searchIndexType("futureType"));
        assertEquals("unknown", Main.searchIndexType(null));
    }
}
