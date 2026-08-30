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
    public static final class ValueHolder {
        public Object getValue() {
            return "legacy";
        }
    }

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
    void coversExtractedExecuteValidationAndResponseHelpers() throws Exception {
        var writer = new PrintWriter(new StringWriter());
        setStatic("connection", null);
        invoke("handleExecute", new Class<?>[]{PrintWriter.class, Object.class, Map.class}, writer, "id", Map.of());
        assertFalse((Boolean) invoke("ensureConnected", new Class<?>[]{PrintWriter.class, Object.class}, writer, "id"));

        var connection = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{java.sql.Connection.class}, (proxy, method, args) ->
                        method.getName().equals("isClosed") ? false : null);
        setStatic("connection", connection);
        assertTrue((Boolean) invoke("ensureConnected", new Class<?>[]{PrintWriter.class, Object.class}, writer, "id"));
        assertNull(invoke("validatedSql", new Class<?>[]{PrintWriter.class, Object.class, Map.class},
                writer, "id", Map.of()));
        assertEquals("select 1", invoke("validatedSql", new Class<?>[]{PrintWriter.class, Object.class, Map.class},
                writer, "id", Map.of("params", Map.of("sql", "select 1"))));

        var readOnlyResult = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{java.sql.ResultSet.class}, new java.lang.reflect.InvocationHandler() {
                    private boolean first = true;
                    public Object invoke(Object proxy, Method method, Object[] args) {
                        return switch (method.getName()) {
                            case "next" -> first ? !(first = false) : false;
                            case "getString" -> "on";
                            default -> null;
                        };
                    }
                });
        var statementForExecute = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{java.sql.Statement.class}, (proxy, method, args) -> switch (method.getName()) {
                    case "execute" -> false;
                    case "executeQuery" -> readOnlyResult;
                    default -> null;
                });
        connection = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{java.sql.Connection.class}, (proxy, method, args) -> switch (method.getName()) {
                    case "isClosed" -> false;
                    case "getAutoCommit" -> false;
                    case "createStatement" -> statementForExecute;
                    default -> null;
                });
        setStatic("connection", connection);
        invoke("handleExecute", new Class<?>[]{PrintWriter.class, Object.class, Map.class}, writer, "id",
                Map.of("params", Map.of("sql", "select 1")));

        var statement = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{java.sql.Statement.class}, (proxy, method, args) -> null);
        setStatic("statementTimeoutMs", 1000L);
        invoke("configureStatementTimeout", new Class<?>[]{java.sql.Statement.class}, statement);
        setStatic("statementTimeoutMs", 0L);
        invoke("sendSqlError", new Class<?>[]{PrintWriter.class, Object.class, java.sql.SQLException.class},
                writer, "id", new java.sql.SQLException("failed", "57014"));
        invoke("sendSqlError", new Class<?>[]{PrintWriter.class, Object.class, java.sql.SQLException.class},
                writer, "id", new java.sql.SQLException("failed", "42000"));
        invoke("writeStatementResponse", new Class<?>[]{PrintWriter.class, Object.class, long.class}, writer, "id", 0L);

        var metadata = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{java.sql.ResultSetMetaData.class}, (proxy, method, args) ->
                        method.getName().equals("getColumnCount") ? 0 : null);
        var resultSet = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{java.sql.ResultSet.class}, (proxy, method, args) -> switch (method.getName()) {
                    case "getMetaData" -> metadata;
                    case "next" -> false;
                    default -> null;
                });
        invoke("writeResultSetResponse", new Class<?>[]{PrintWriter.class, Object.class, java.sql.ResultSet.class, long.class},
                writer, "id", resultSet, 0L);
        invoke("readResultRow", new Class<?>[]{java.sql.ResultSet.class, int.class}, resultSet, 0);
        setStatic("connection", null);
    }

    @Test
    void configuresAndVerifiesReadOnlyTransactionsForWriteCapableCredentials() throws Exception {
        List<String> calls = new ArrayList<>();
        var readOnlyResult = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{java.sql.ResultSet.class}, new java.lang.reflect.InvocationHandler() {
                    private boolean first = true;
                    public Object invoke(Object proxy, Method method, Object[] args) {
                        return switch (method.getName()) {
                            case "next" -> first ? !(first = false) : false;
                            case "getString" -> "on";
                            default -> null;
                        };
                    }
                });
        var statement = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{java.sql.Statement.class}, (proxy, method, args) -> {
                    if (method.getName().equals("executeQuery")) {
                        calls.add(String.valueOf(args[0]));
                        return readOnlyResult;
                    }
                    return null;
                });
        var connection = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{java.sql.Connection.class}, (proxy, method, args) -> {
                    calls.add(method.getName() + (args == null ? "" : List.of(args).toString()));
                    return switch (method.getName()) {
                        case "createStatement" -> statement;
                        case "getAutoCommit" -> false;
                        default -> null;
                    };
                });
        setStatic("connection", connection);

        invoke("configureReadOnlyConnection", new Class<?>[]{});

        assertTrue(calls.stream().anyMatch(value -> value.startsWith("setReadOnly[true]")));
        assertTrue(calls.stream().anyMatch(value -> value.startsWith("setAutoCommit[false]")));
        assertTrue(calls.contains("SHOW transaction_read_only"));
        assertTrue(calls.contains("rollback"));
        setStatic("connection", null);
    }

    @Test
    void coversArgumentAndRequestDispatchers() throws Exception {
        invoke("configureArguments", new Class<?>[]{String[].class}, (Object) new String[]{
                "--backend", "mongodb", "--url", "mongodb://localhost", "--user", "safe",
                "--password-stdin", "--idle-timeout-seconds", "2", "--statement-timeout-ms", "20",
                "--max-rows", "3", "--max-result-bytes", "40", "--verbose"});
        invoke("validateArguments", new Class<?>[]{});
        assertFalse((Boolean) invoke("isJdbcPasswordMissing", new Class<?>[]{}));
        setStatic("backend", "jdbc");
        setStatic("password", "safe");
        assertFalse((Boolean) invoke("isJdbcPasswordMissing", new Class<?>[]{}));
        var writer = new PrintWriter(new StringWriter());
        var running = Main.class.getDeclaredField("RUNNING");
        running.setAccessible(true);
        ((java.util.concurrent.atomic.AtomicBoolean) running.get(null)).set(true);
        invoke("processRequests", new Class<?>[]{java.io.BufferedReader.class, PrintWriter.class},
                new java.io.BufferedReader(new java.io.StringReader("")), writer);
        setStatic("connection", null);
        invoke("closeJdbcBackend", new Class<?>[]{});
        invoke("dispatchRequest", new Class<?>[]{PrintWriter.class, Map.class, Object.class, String.class},
                writer, Map.of(), "id", "ping");
        invoke("dispatchRequest", new Class<?>[]{PrintWriter.class, Map.class, Object.class, String.class},
                writer, Map.of(), "id", "unknown");
        ((java.util.concurrent.atomic.AtomicBoolean) running.get(null)).set(true);
        invoke("dispatchRequest", new Class<?>[]{PrintWriter.class, Map.class, Object.class, String.class},
                writer, Map.of(), "id", "shutdown");
        ((java.util.concurrent.atomic.AtomicBoolean) running.get(null)).set(true);
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
    void coversMongoReadHandlersWithEmptyConnectedProxies() throws Exception {
        var writer = new PrintWriter(new StringWriter());
        java.util.function.Supplier<com.mongodb.client.MongoCursor<org.bson.Document>> documentCursor = () -> {
            boolean[] available = {true};
            return (com.mongodb.client.MongoCursor<org.bson.Document>) Proxy.newProxyInstance(
                    MainTest.class.getClassLoader(),
                    new Class<?>[]{com.mongodb.client.MongoCursor.class},
                    (proxy, method, args) -> switch (method.getName()) {
                        case "hasNext" -> available[0];
                        case "next" -> {
                            available[0] = false;
                            yield new org.bson.Document("name", "Ada")
                                    .append("age", 42).append("active", true);
                        }
                        case "close" -> null;
                        default -> null;
                    });
        };
        java.util.function.Supplier<com.mongodb.client.MongoCursor<org.bson.BsonValue>> valueCursor = () -> {
            boolean[] available = {true};
            return (com.mongodb.client.MongoCursor<org.bson.BsonValue>) Proxy.newProxyInstance(
                    MainTest.class.getClassLoader(),
                    new Class<?>[]{com.mongodb.client.MongoCursor.class},
                    (proxy, method, args) -> switch (method.getName()) {
                        case "hasNext" -> available[0];
                        case "next" -> {
                            available[0] = false;
                            yield new org.bson.BsonString("Ada");
                        }
                        case "close" -> null;
                        default -> null;
                    });
        };
        var stringCursor = (com.mongodb.client.MongoCursor<String>) Proxy.newProxyInstance(
                MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoCursor.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "hasNext" -> false;
                    case "close" -> null;
                    default -> null;
                });
        var collectionNames = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.ListCollectionNamesIterable.class},
                (proxy, method, args) -> method.getName().equals("iterator") ? stringCursor : null);
        var indexes = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.ListIndexesIterable.class},
                (proxy, method, args) -> method.getName().equals("iterator") ? documentCursor.get() : null);
        var searchIndexes = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.ListSearchIndexesIterable.class},
                (proxy, method, args) -> method.getName().equals("iterator") ? documentCursor.get() : null);
        var find = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.FindIterable.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "iterator" -> documentCursor.get();
                    case "limit", "projection", "sort" -> proxy;
                    default -> null;
                });
        var aggregate = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.AggregateIterable.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "iterator" -> documentCursor.get();
                    case "allowDiskUse" -> proxy;
                    default -> null;
                });
        var distinct = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.DistinctIterable.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "iterator" -> valueCursor.get();
                    default -> null;
                });
        var collection = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoCollection.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "listIndexes" -> indexes;
                    case "listSearchIndexes" -> searchIndexes;
                    case "find" -> find;
                    case "aggregate" -> aggregate;
                    case "distinct" -> distinct;
                    case "countDocuments" -> 0L;
                    default -> null;
                });
        var database = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoDatabase.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getCollection" -> collection;
                    case "listCollectionNames" -> collectionNames;
                    case "runCommand" -> new org.bson.Document();
                    default -> null;
                });
        var client = Proxy.newProxyInstance(MainTest.class.getClassLoader(),
                new Class<?>[]{com.mongodb.client.MongoClient.class},
                (proxy, method, args) -> method.getName().equals("getDatabase") ? database : null);

        setStatic("backend", "mongodb");
        setStatic("mongoClient", client);
        Map<String, Object> params = new HashMap<>();
        params.put("database", "app");
        params.put("collection", "safe");
        params.put("field", "name");
        params.put("filter", Map.of("active", true));
        params.put("projection", Map.of("name", 1));
        params.put("sort", Map.of("name", 1));
        params.put("limit", 1);
        params.put("sample_size", 1);
        params.put("examples", 1);
        params.put("redact_fields", List.of("name"));
        params.put("pipeline", List.of(Map.of("$match", Map.of("active", true))));
        setStatic("statementTimeoutMs", 100L);
        setStatic("maxRows", 10L);
        setStatic("maxResultBytes", 100_000L);
        for (String name : List.of("handleListCollectionIndexes", "handleFindDocuments", "handleAggregateDocuments",
                "handleDistinctDocuments", "handleCountDocuments", "handleExplainDocuments", "handleProfileDocumentField",
                "handleDiscoverDocumentSchema", "handleGenerateDocumentFixture")) {
            try {
                invoke(name, new Class<?>[]{PrintWriter.class, Object.class, Map.class}, writer, "id", Map.of("params", params));
            } catch (InvocationTargetException ignored) {
                // Empty driver proxies are only used to exercise request validation and bounded loops.
            }
        }
        setStatic("maxResultBytes", 1L);
        for (String name : List.of("handleFindDocuments", "handleDistinctDocuments", "handleGenerateDocumentFixture")) {
            try {
                invoke(name, new Class<?>[]{PrintWriter.class, Object.class, Map.class}, writer, "limit", Map.of("params", params));
            } catch (InvocationTargetException ignored) {
                // The bounded response path is expected to terminate the handler early.
            }
        }
        setStatic("mongoClient", null);
        setStatic("statementTimeoutMs", 0L);
        setStatic("maxRows", Long.MAX_VALUE);
        setStatic("maxResultBytes", Long.MAX_VALUE);
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
    void coversJdbcConnectWhenExistingConnectionIsValid() throws Exception {
        var connection = Proxy.newProxyInstance(MainTest.class.getClassLoader(), new Class<?>[]{java.sql.Connection.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "isClosed" -> false;
                    case "isValid" -> true;
                    default -> null;
                });
        setStatic("backend", "jdbc");
        setStatic("connection", connection);
        invoke("handleConnect", new Class<?>[]{PrintWriter.class, Object.class}, new PrintWriter(new StringWriter()), "id");
        setStatic("connection", null);
    }

    @Test
    void coversJdbcReconnectWhenExistingConnectionIsStale() throws Exception {
        var connection = Proxy.newProxyInstance(MainTest.class.getClassLoader(), new Class<?>[]{java.sql.Connection.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "isClosed" -> false;
                    case "isValid" -> false;
                    case "close" -> null;
                    default -> null;
                });
        setStatic("backend", "jdbc");
        setStatic("connection", connection);
        setStatic("databaseUrl", "jdbc:invalid://127.0.0.1:1/app");
        try {
            invoke("handleConnect", new Class<?>[]{PrintWriter.class, Object.class},
                    new PrintWriter(new StringWriter()), "id");
        } catch (InvocationTargetException ignored) {
            // DriverManager failure is expected; the stale-connection branch is the subject of this test.
        }
        var failingConnection = Proxy.newProxyInstance(MainTest.class.getClassLoader(), new Class<?>[]{java.sql.Connection.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "isClosed" -> false;
                    case "isValid", "close" -> throw new java.sql.SQLException("stale connection");
                    default -> null;
                });
        setStatic("connection", failingConnection);
        try {
            invoke("handleConnect", new Class<?>[]{PrintWriter.class, Object.class},
                    new PrintWriter(new StringWriter()), "id");
        } catch (InvocationTargetException ignored) {
            // Both validation and close failures are intentionally exercised.
        }
        setStatic("connection", null);
    }

    @Test
    void rejectsNonPostgresqlJdbcUrlsBeforeConnecting() throws Exception {
        setStatic("backend", "jdbc");
        setStatic("databaseUrl", "jdbc:h2:mem:test");
        try {
            invoke("connectBackend", new Class<?>[]{});
            assertTrue(false, "Non-PostgreSQL JDBC URLs must be rejected");
        } catch (InvocationTargetException error) {
            assertInstanceOf(java.sql.SQLException.class, error.getCause());
        } finally {
            setStatic("databaseUrl", null);
        }
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
    void coversPostgresObjectConversions() throws Exception {
        var clob = new javax.sql.rowset.serial.SerialClob(new char[]{'s', 'a', 'f', 'e'});
        assertEquals("safe", invoke("convertPgObject", new Class<?>[]{Object.class}, clob));
        var array = Proxy.newProxyInstance(MainTest.class.getClassLoader(), new Class<?>[]{java.sql.Array.class},
                (proxy, method, args) -> method.getName().equals("getArray") ? new Object[]{"one", "two"} : null);
        assertEquals(List.of("one", "two"), invoke("convertPgObject", new Class<?>[]{Object.class}, array));
        assertEquals("legacy", invoke("convertPgObject", new Class<?>[]{Object.class}, new ValueHolder()));
        var pgObject = new org.postgresql.util.PGobject();
        pgObject.setType("jsonb");
        pgObject.setValue("{\"safe\":true}");
        assertEquals(Map.of("safe", true), invoke("convertPgObject", new Class<?>[]{Object.class}, pgObject));
    }

    @Test
    void initializesLogWriterInTemporaryHome() throws Exception {
        String previous = System.getProperty("user.home");
        var home = java.nio.file.Files.createTempDirectory("safeselect-home");
        System.setProperty("user.home", home.toString());
        invoke("initializeLogWriter", new Class<?>[]{});
        var logWriterField = Main.class.getDeclaredField("logWriter");
        logWriterField.setAccessible(true);
        if (logWriterField.get(null) instanceof PrintWriter writer) {
            writer.close();
        }
        var rotatedHome = java.nio.file.Files.createTempDirectory("safeselect-rotated-home");
        var logDirectory = java.nio.file.Files.createDirectories(rotatedHome.resolve(".local/state/safeselect/logs"));
        var activeLog = logDirectory.resolve("sidecar.log");
        try (var file = new java.io.RandomAccessFile(activeLog.toFile(), "rw")) {
            file.setLength(10L * 1024 * 1024);
        }
        System.setProperty("user.home", rotatedHome.toString());
        invoke("initializeLogWriter", new Class<?>[]{});
        if (logWriterField.get(null) instanceof PrintWriter writer) {
            writer.close();
        }
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
        for (org.bson.BsonValue value : List.of(
                new org.bson.BsonNull(), new org.bson.BsonUndefined(), new org.bson.BsonString("text"),
                new org.bson.BsonBoolean(true), new org.bson.BsonInt32(1), new org.bson.BsonInt64(2L),
                new org.bson.BsonDouble(3.0), new org.bson.BsonDecimal128(new org.bson.types.Decimal128(java.math.BigDecimal.ONE)),
                new org.bson.BsonObjectId(), new org.bson.BsonDateTime(4L), new org.bson.BsonTimestamp(5),
                new org.bson.BsonRegularExpression("safe", "i"), new org.bson.BsonBinary(new byte[]{1, 2}),
                new org.bson.BsonSymbol("symbol"))) {
            assertTrue(invoke("convertBsonValue", new Class<?>[]{org.bson.BsonValue.class}, value) != null
                    || value.isNull() || value.getBsonType() == org.bson.BsonType.UNDEFINED);
        }

        Map<String, Object> nested = new HashMap<>(Map.of("user", new HashMap<>(Map.of("token", "secret", "name", "Ada"))));
        invoke("redactValue", new Class<?>[]{Object.class, String.class, List.class}, nested, "", List.of("user.token"));
        assertEquals("[REDACTED]", ((Map<?, ?>) nested.get("user")).get("token"));
        var list = new ArrayList<Object>();
        list.add(new HashMap<>(Map.of("token", "another")));
        invoke("redactValue", new Class<?>[]{Object.class, String.class, List.class}, list, "", List.of("token"));
        assertEquals("[REDACTED]", ((Map<?, ?>) list.get(0)).get("token"));
        assertEquals("Ada", invoke("resolvePath", new Class<?>[]{Object.class, String.class}, nested, "user.name"));
        assertEquals("Ada", invoke("resolvePath", new Class<?>[]{Object.class, String.class},
                new org.bson.Document("user", new org.bson.Document("name", "Ada")), "user.name"));
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
