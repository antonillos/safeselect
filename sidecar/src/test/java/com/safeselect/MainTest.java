package com.safeselect;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.lang.reflect.Method;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Proxy;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import org.junit.jupiter.api.Test;

class MainTest {
    private static Object invoke(String name, Class<?>[] types, Object... args) throws Exception {
        Method method = Main.class.getDeclaredMethod(name, types);
        method.setAccessible(true);
        return method.invoke(null, args);
    }

    private static void setStatic(String name, Object value) throws Exception {
        var field = Main.class.getDeclaredField(name);
        field.setAccessible(true);
        field.set(null, value);
    }

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

    @Test
    void classifiesLocalSearchNotEnabledAsUnsupported() {
        assertEquals(true, Main.isSearchUnsupported(59, "command not found"));
        assertEquals(true, Main.isSearchUnsupported(31082, "SearchNotEnabled"));
        assertEquals(false, Main.isSearchUnsupported(13, "not authorized"));
    }

    @Test
    void treatsTheImplicitIdIndexAsUnique() {
        assertEquals(true, Main.isClassicIndexUnique("_id_", false));
        assertEquals(true, Main.isClassicIndexUnique("email_1", true));
        assertEquals(false, Main.isClassicIndexUnique("email_1", false));
    }

    @Test
    void coversSmallSerializationAndParameterHelpers() throws Exception {
        Map<String, Object> params = new HashMap<>();
        params.put("name", "safe");
        params.put("limit", 7);
        assertEquals("safe", invoke("stringParam", new Class<?>[]{Map.class, String.class}, params, "name"));
        assertNull(invoke("stringParam", new Class<?>[]{Map.class, String.class}, params, "missing"));
        assertEquals(7L, invoke("numberParam", new Class<?>[]{Map.class, String.class, long.class}, params, "limit", 3L));
        assertEquals(3L, invoke("numberParam", new Class<?>[]{Map.class, String.class, long.class}, params, "missing", 3L));
        assertEquals("12ms", invoke("formatElapsed", new Class<?>[]{long.class}, 12L));
        assertEquals("1.2s", invoke("formatElapsed", new Class<?>[]{long.class}, 1200L));
        assertEquals("1m 2s", invoke("formatElapsed", new Class<?>[]{long.class}, 62000L));
        assertEquals("IllegalStateException: failed", invoke("summarizeException", new Class<?>[]{Throwable.class}, new IllegalStateException("failed")));
        assertEquals("IllegalStateException", invoke("summarizeException", new Class<?>[]{Throwable.class}, new IllegalStateException()));
        assertInstanceOf(Map.class, invoke("toDocument", new Class<?>[]{Object.class}, Map.of("a", 1)));
        assertEquals(Map.of("a", 1), invoke("convertBsonValue", new Class<?>[]{Object.class}, new org.bson.Document("a", 1)));
    }

    @Test
    void coversMongoGuardAndLoggingHelpers() throws Exception {
        StringWriter output = new StringWriter();
        assertFalse((Boolean) invoke("rejectMongoJavaScript", new Class<?>[]{PrintWriter.class, Object.class, Object[].class},
                new PrintWriter(output), "id", new Object[]{Map.of("ok", true)}));
        assertTrue((Boolean) invoke("rejectMongoJavaScript", new Class<?>[]{PrintWriter.class, Object.class, Object[].class},
                new PrintWriter(output), "id", new Object[]{Map.of("$where", "x")}));
        invoke("sendResponse", new Class<?>[]{PrintWriter.class, Object.class, Object.class, Object.class},
                new PrintWriter(output), "id", Map.of("ok", true), null);
        assertNull(invoke("convertPgObject", new Class<?>[]{Object.class}, new Object[]{null}));
        setStatic("verboseMode", true);
        setStatic("logWriter", new PrintWriter(new StringWriter()));
        invoke("log", new Class<?>[]{String.class}, "verbose");
        setStatic("verboseMode", false);
        setStatic("logWriter", null);
    }

    @Test
    void coversDisconnectedMongoOperationGuards() throws Exception {
        setStatic("backend", "mongodb");
        setStatic("mongoClient", null);
        var writer = new PrintWriter(new StringWriter());
        assertThrows(InvocationTargetException.class, () ->
                invoke("ensureMongoConnected", new Class<?>[]{PrintWriter.class, Object.class}, writer, "id"));
    }

    @Test
    void coversMongoHandlersWhenAlreadyDisconnected() throws Exception {
        setStatic("backend", "mongodb");
        var writer = new PrintWriter(new StringWriter());
        invoke("handleDisconnect", new Class<?>[]{PrintWriter.class, Object.class}, writer, "id");
        invoke("handleListDatabases", new Class<?>[]{PrintWriter.class, Object.class}, writer, "id");
        invoke("handleVerifyDocumentConnection", new Class<?>[]{PrintWriter.class, Object.class}, writer, "id");
        invoke("handleListCollections", new Class<?>[]{PrintWriter.class, Object.class, Map.class}, writer, "id", Map.of());
        for (String name : List.of("handleListCollectionIndexes", "handleGetDatabaseStats", "handleGetCollectionStats",
                "handleFindDocuments", "handleAggregateDocuments", "handleDistinctDocuments", "handleCountDocuments",
                "handleExplainDocuments", "handleProfileDocumentField", "handleDiscoverDocumentSchema",
                "handleGenerateDocumentFixture")) {
            invoke(name, new Class<?>[]{PrintWriter.class, Object.class, Map.class}, writer, "id", Map.of());
        }
        setStatic("backend", "jdbc");
    }

    @Test
    void coversMongoDatabaseListingSuccessPath() throws Exception {
        var cursor = (com.mongodb.client.MongoCursor<String>) Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoCursor.class}, new java.lang.reflect.InvocationHandler() {
                    private final java.util.Iterator<String> values = List.of("admin", "app").iterator();
                    public Object invoke(Object proxy, Method method, Object[] args) {
                        return switch (method.getName()) {
                            case "hasNext" -> values.hasNext();
                            case "next" -> values.next();
                            case "close" -> null;
                            default -> null;
                        };
                    }
                });
        var names = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.ListDatabasesIterable.class},
                (proxy, method, args) -> method.getName().equals("iterator")
                        ? cursor : null);
        var client = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoClient.class},
                (proxy, method, args) -> method.getName().equals("listDatabaseNames") ? names : null);
        setStatic("backend", "mongodb");
        setStatic("mongoClient", client);
        invoke("handleListDatabases", new Class<?>[]{PrintWriter.class, Object.class}, new PrintWriter(new StringWriter()), "id");
        setStatic("mongoClient", null);
        setStatic("backend", "jdbc");
    }

    @Test
    void coversMongoStatisticsSuccessPaths() throws Exception {
        var stats = new org.bson.Document()
                .append("collections", 2).append("views", 0).append("objects", 4)
                .append("avgObjSize", 10).append("dataSize", 40).append("storageSize", 80)
                .append("indexes", 1).append("indexSize", 8).append("totalSize", 88)
                .append("count", 4).append("size", 40).append("nindexes", 1).append("totalIndexSize", 8);
        var database = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoDatabase.class},
                (proxy, method, args) -> method.getName().equals("runCommand") ? stats : null);
        var client = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoClient.class},
                (proxy, method, args) -> method.getName().equals("getDatabase") ? database : null);
        setStatic("backend", "mongodb");
        setStatic("mongoClient", client);
        var request = Map.of("params", Map.of("database", "app", "collection", "safe"));
        var writer = new PrintWriter(new StringWriter());
        invoke("handleGetDatabaseStats", new Class<?>[]{PrintWriter.class, Object.class, Map.class}, writer, "id", request);
        invoke("handleGetCollectionStats", new Class<?>[]{PrintWriter.class, Object.class, Map.class}, writer, "id", request);
        setStatic("mongoClient", null);
        setStatic("backend", "jdbc");
    }

    @Test
    void coversMongoCollectionListingSuccessPath() throws Exception {
        var cursor = (com.mongodb.client.MongoCursor<String>) Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoCursor.class}, new java.lang.reflect.InvocationHandler() {
                    private final java.util.Iterator<String> values = List.of("safe_docs").iterator();
                    public Object invoke(Object proxy, Method method, Object[] args) {
                        return switch (method.getName()) {
                            case "hasNext" -> values.hasNext();
                            case "next" -> values.next();
                            case "close" -> null;
                            default -> null;
                        };
                    }
                });
        var collections = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.ListCollectionNamesIterable.class},
                (proxy, method, args) -> method.getName().equals("iterator") ? cursor : null);
        var database = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoDatabase.class},
                (proxy, method, args) -> method.getName().equals("listCollectionNames") ? collections : null);
        var client = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoClient.class},
                (proxy, method, args) -> method.getName().equals("getDatabase") ? database : null);
        setStatic("backend", "mongodb");
        setStatic("mongoClient", client);
        invoke("handleListCollections", new Class<?>[]{PrintWriter.class, Object.class, Map.class},
                new PrintWriter(new StringWriter()), "id", Map.of("params", Map.of("database", "app")));
        setStatic("mongoClient", null);
        setStatic("backend", "jdbc");
    }

    @Test
    void coversEmptyDocumentStreamingAndJdbcTimeout() throws Exception {
        var writer = new PrintWriter(new StringWriter());
        var cursor = (com.mongodb.client.MongoCursor<org.bson.Document>) Proxy.newProxyInstance(
                MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoCursor.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "hasNext" -> false;
                    case "close" -> null;
                    default -> throw new UnsupportedOperationException(method.getName());
                });
        invoke("sendDocumentIterable", new Class<?>[]{PrintWriter.class, Object.class, com.mongodb.client.MongoCursor.class,
                long.class, String.class, String.class}, writer, "id", cursor, System.currentTimeMillis(), "documents", "count");

        var statement = Proxy.newProxyInstance(MainTest.class.getClassLoader(), new Class<?>[]{java.sql.Statement.class},
                (proxy, method, args) -> method.getName().equals("execute") ? true : null);
        var connection = Proxy.newProxyInstance(MainTest.class.getClassLoader(), new Class<?>[]{java.sql.Connection.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "isClosed" -> false;
                    case "createStatement" -> statement;
                    default -> null;
                });
        setStatic("connection", connection);
        setStatic("statementTimeoutMs", 100L);
        invoke("applyStatementTimeout", new Class<?>[]{});
        setStatic("connection", null);
        setStatic("statementTimeoutMs", 0L);
    }

    @Test
    void coversJdbcDisconnectSuccessPath() throws Exception {
        var connection = Proxy.newProxyInstance(MainTest.class.getClassLoader(), new Class<?>[]{java.sql.Connection.class},
                (proxy, method, args) -> method.getName().equals("isClosed") ? false : null);
        setStatic("backend", "jdbc");
        setStatic("connection", connection);
        invoke("handleDisconnect", new Class<?>[]{PrintWriter.class, Object.class}, new PrintWriter(new StringWriter()), "id");
        setStatic("connection", null);
    }

    @Test
    void coversMongoBackendConnectionSetup() throws Exception {
        setStatic("backend", "mongodb");
        setStatic("databaseUrl", "mongodb://localhost:27017/test");
        setStatic("password", "secret");
        invoke("connectBackend", new Class<?>[]{});
        invoke("handleConnect", new Class<?>[]{PrintWriter.class, Object.class}, new PrintWriter(new StringWriter()), "id");
        Object client = invoke("convertPgObject", new Class<?>[]{Object.class}, "plain");
        assertEquals("plain", client);
        setStatic("mongoClient", null);
        setStatic("backend", "jdbc");
    }

    @Test
    void initializesLogWriterInTemporaryHome() throws Exception {
        String previous = System.getProperty("user.home");
        var home = java.nio.file.Files.createTempDirectory("safeselect-home");
        System.setProperty("user.home", home.toString());
        invoke("initializeLogWriter", new Class<?>[]{});
        if (previous == null) System.clearProperty("user.home"); else System.setProperty("user.home", previous);
    }

    @Test
    void coversIdleTimerDisconnectNotification() throws Exception {
        var connection = Proxy.newProxyInstance(MainTest.class.getClassLoader(), new Class<?>[]{java.sql.Connection.class},
                (proxy, method, args) -> method.getName().equals("isClosed") ? false : null);
        setStatic("connection", connection);
        setStatic("idleTimeoutMs", 0L);
        var lastActivityField = Main.class.getDeclaredField("lastActivityMs");
        lastActivityField.setAccessible(true);
        var lastActivity = (java.util.concurrent.atomic.AtomicLong) lastActivityField.get(null);
        lastActivity.set(System.currentTimeMillis() - 2000);
        var runningField = Main.class.getDeclaredField("RUNNING");
        runningField.setAccessible(true);
        var running = (java.util.concurrent.atomic.AtomicBoolean) runningField.get(null);
        running.set(true);
        invoke("startIdleTimer", new Class<?>[]{PrintWriter.class}, new PrintWriter(new StringWriter()));
        Thread.sleep(1100L);
        running.set(false);
        setStatic("connection", null);
    }

    @Test
    void coversBsonConversionAndNestedValueHelpers() throws Exception {
        Object converted = invoke("convertBsonValue", new Class<?>[]{org.bson.BsonValue.class},
                new org.bson.BsonDocument("name", new org.bson.BsonString("safe")));
        assertEquals(Map.of("name", "safe"), converted);
        assertEquals(List.of(1, "two"), invoke("convertBsonValue", new Class<?>[]{org.bson.BsonValue.class},
                new org.bson.BsonArray(List.of(new org.bson.BsonInt32(1), new org.bson.BsonString("two")))));

        Map<String, Object> nested = new HashMap<>(Map.of("user", new HashMap<>(Map.of("token", "secret", "name", "Ada"))));
        invoke("redactValue", new Class<?>[]{Object.class, String.class, List.class}, nested, "", List.of("user.token"));
        assertEquals("[REDACTED]", ((Map<?, ?>) nested.get("user")).get("token"));
        var list = new ArrayList<Object>();
        list.add(new HashMap<>(Map.of("token", "another")));
        invoke("redactValue", new Class<?>[]{Object.class, String.class, List.class}, list, "", List.of("token"));
        assertEquals("[REDACTED]", ((Map<?, ?>) list.get(0)).get("token"));
        assertEquals("Ada", invoke("resolvePath", new Class<?>[]{Object.class, String.class}, nested, "user.name"));
        assertTrue(invoke("resolvePath", new Class<?>[]{Object.class, String.class}, nested, "user.missing").toString().contains("INSTANCE"));
    }

    @Test
    void coversBoundedResultsAndFieldCollection() throws Exception {
        var values = new ArrayList<Object>();
        var maxRowsField = Main.class.getDeclaredField("maxResultBytes");
        maxRowsField.setAccessible(true);
        maxRowsField.setLong(null, 1000L);
        assertTrue((Boolean) invoke("appendBounded", new Class<?>[]{List.class, Object.class}, values, Map.of("id", 1)));
        maxRowsField.setLong(null, 1L);
        assertFalse((Boolean) invoke("appendBounded", new Class<?>[]{List.class, Object.class}, values, "too large"));
        maxRowsField.setLong(null, Long.MAX_VALUE);
        Map<String, Object> fields = new HashMap<>();
        invoke("collectFields", new Class<?>[]{String.class, Object.class, Map.class, long.class}, "", Map.of("name", "Ada"), fields, 2L);
        invoke("collectFields", new Class<?>[]{String.class, Object.class, Map.class, long.class}, "items", List.of("one", "two"), fields, 2L);
        assertTrue(fields.containsKey("name"));
    }
}
