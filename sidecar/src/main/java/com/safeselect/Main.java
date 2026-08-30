package com.safeselect;

import tools.jackson.databind.ObjectMapper;
import com.mongodb.client.FindIterable;
import com.mongodb.client.AggregateIterable;
import com.mongodb.client.MongoClient;
import com.mongodb.client.MongoClients;
import com.mongodb.client.MongoCollection;
import com.mongodb.client.MongoCursor;
import com.mongodb.client.MongoDatabase;
import com.mongodb.ReadPreference;
import com.mongodb.MongoCommandException;
import org.bson.BsonArray;
import org.bson.BsonBinary;
import org.bson.BsonDocument;
import org.bson.BsonType;
import org.bson.BsonValue;
import org.bson.Document;

import java.io.*;
import java.sql.*;
import java.net.URLEncoder;
import java.time.Instant;
import java.util.*;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;

public class Main {
    private static final ObjectMapper MAPPER = new ObjectMapper();
    private static final AtomicBoolean RUNNING = new AtomicBoolean(true);
    private static final long MAX_LOG_BYTES = 10L * 1024 * 1024;
    private static final Set<BsonType> SCALAR_BSON_TYPES = Set.of(
            BsonType.STRING, BsonType.BOOLEAN, BsonType.INT32, BsonType.INT64,
            BsonType.DOUBLE, BsonType.DECIMAL128, BsonType.OBJECT_ID, BsonType.DATE_TIME,
            BsonType.TIMESTAMP);
    private static final Map<BsonType, java.util.function.Function<BsonValue, Object>> BSON_SCALAR_CONVERTERS = Map.ofEntries(
            Map.entry(BsonType.STRING, value -> value.asString().getValue()),
            Map.entry(BsonType.BOOLEAN, value -> value.asBoolean().getValue()),
            Map.entry(BsonType.INT32, value -> value.asInt32().getValue()),
            Map.entry(BsonType.INT64, value -> value.asInt64().getValue()),
            Map.entry(BsonType.DOUBLE, value -> value.asDouble().getValue()),
            Map.entry(BsonType.DECIMAL128, value -> value.asDecimal128().getValue().bigDecimalValue()),
            Map.entry(BsonType.OBJECT_ID, value -> value.asObjectId().getValue().toHexString()),
            Map.entry(BsonType.DATE_TIME, value -> value.asDateTime().getValue()),
            Map.entry(BsonType.TIMESTAMP, value -> value.asTimestamp().getValue()));
    private static Connection connection;
    private static MongoClient mongoClient;
    private static String backend;
    private static String driverClass;
    private static String databaseUrl;
    private static String user;
    private static String password;
    private static long idleTimeoutMs = 0;
    private static long statementTimeoutMs = 0;
    private static long maxRows = Long.MAX_VALUE;
    private static long maxResultBytes = Long.MAX_VALUE;
    private static boolean verboseMode = false;
    private static boolean passwordStdin = false;
    private static final AtomicLong lastActivityMs = new AtomicLong(System.currentTimeMillis());
    private static PrintWriter logWriter;

    @FunctionalInterface
    private interface ArgumentSetter { void set(String value); }

    @FunctionalInterface
    private interface RequestHandler { void handle(PrintWriter writer, Object id, Map<String, Object> request) throws Exception; }

    private static final Map<String, ArgumentSetter> ARGUMENTS = Map.ofEntries(
            Map.entry("--backend", value -> backend = value),
            Map.entry("--driver", value -> driverClass = value),
            Map.entry("--url", value -> databaseUrl = value),
            Map.entry("--user", value -> user = value),
            Map.entry("--idle-timeout-seconds", value -> idleTimeoutMs = Long.parseLong(value) * 1000),
            Map.entry("--statement-timeout-ms", value -> statementTimeoutMs = Long.parseLong(value)),
            Map.entry("--max-rows", value -> maxRows = Long.parseLong(value)),
            Map.entry("--max-result-bytes", value -> maxResultBytes = Long.parseLong(value)));

    private static final Map<String, RequestHandler> REQUEST_HANDLERS = Map.ofEntries(
            Map.entry("ping", (w, id, r) -> sendResponse(w, id, "pong", null)),
            Map.entry("execute", Main::handleExecute),
            Map.entry("list_databases", (w, id, r) -> handleListDatabases(w, id)),
            Map.entry("verify_document_connection", (w, id, r) -> handleVerifyDocumentConnection(w, id)),
            Map.entry("list_collections", Main::handleListCollections),
            Map.entry("list_collection_indexes", Main::handleListCollectionIndexes),
            Map.entry("get_database_stats", Main::handleGetDatabaseStats),
            Map.entry("get_collection_stats", Main::handleGetCollectionStats),
            Map.entry("find_documents", Main::handleFindDocuments),
            Map.entry("aggregate_documents", Main::handleAggregateDocuments),
            Map.entry("distinct_documents", Main::handleDistinctDocuments),
            Map.entry("count_documents", Main::handleCountDocuments),
            Map.entry("explain_documents", Main::handleExplainDocuments),
            Map.entry("profile_document_field", Main::handleProfileDocumentField),
            Map.entry("discover_document_schema", Main::handleDiscoverDocumentSchema),
            Map.entry("generate_document_fixture", Main::handleGenerateDocumentFixture),
            Map.entry("disconnect", (w, id, r) -> handleDisconnect(w, id)),
            Map.entry("connect", (w, id, r) -> handleConnect(w, id)));

    private static void initializeLogWriter() throws IOException {
        String logDir = System.getProperty("user.home") + "/.local/state/safeselect/logs";
        File logDirectory = new File(logDir);
        if (!logDirectory.exists()) {
            logDirectory.mkdirs();
        }

        File activeLog = new File(logDirectory, "sidecar.log");
        if (activeLog.exists() && activeLog.length() >= MAX_LOG_BYTES) {
            File rotatedLog = new File(logDirectory, "sidecar.log.1");
            if (rotatedLog.exists() && !rotatedLog.delete()) {
                throw new IOException("Failed to delete rotated log: " + rotatedLog.getAbsolutePath());
            }
            if (!activeLog.renameTo(rotatedLog)) {
                throw new IOException("Failed to rotate log file: " + activeLog.getAbsolutePath());
            }
        }

        logWriter = new PrintWriter(new FileWriter(activeLog, true));
    }

    private static void log(String message) {
        if (!verboseMode) {
            return;
        }
        String timestamp = Instant.now().toString();
        String logLine = "[" + timestamp + "] " + message;
        System.err.println(logLine);
        if (logWriter != null) {
            logWriter.println(logLine);
            logWriter.flush();
        }
    }

    private static void error(String message) {
        String timestamp = Instant.now().toString();
        String logLine = "[" + timestamp + "] ERROR: " + message;
        System.err.println(logLine);
        if (logWriter != null) {
            logWriter.println(logLine);
            logWriter.flush();
        }
    }

    private static String summarizeException(Throwable throwable) {
        Throwable current = throwable;
        while (current.getCause() != null && current.getCause() != current) {
            current = current.getCause();
        }
        final var type = current.getClass().getSimpleName();
        final var message = current.getMessage();
        if (message == null || message.isBlank()) {
            return type;
        }
        return type + ": " + message;
    }

    public static void main(String[] args) throws Exception {
        configureArguments(args);
        validateArguments();
        runSidecar();
    }

    private static void validateArguments() { if (databaseUrl == null || user == null || !passwordStdin || ("jdbc".equals(backend) && driverClass == null)) {
            error("Usage: --backend <jdbc|mongodb> [--driver <class>] --url <url> --user <name> --password-stdin [--idle-timeout-seconds <sec>] [--statement-timeout-ms <ms>] [--max-rows <n>] [--max-result-bytes <n>]");
            System.exit(1);
        }
    }

    private static void runSidecar() throws Exception {
        configureLogging();
        BufferedReader reader = new BufferedReader(new InputStreamReader(System.in));
        PrintWriter writer = new PrintWriter(new OutputStreamWriter(System.out));
        password = reader.readLine();
        validatePassword();
        configureIdleTimer(writer);
        try {
            connectBackend();
            writer.println("ready");
            writer.flush();
            processRequests(reader, writer);
            closeBackends();
        } catch (Exception e) {
            error("Fatal error: " + summarizeException(e));
            System.exit(1);
        }
    }

    private static void configureLogging() throws IOException {
        if (verboseMode) {
            initializeLogWriter();
            log("Starting sidecar");
        }
    }

    private static void configureIdleTimer(PrintWriter writer) {
        if (idleTimeoutMs > 0) {
            startIdleTimer(writer);
        }
    }

    private static void validatePassword() {
        if (isJdbcPasswordMissing()) {
            error("Password required on stdin");
            System.exit(1);
        }
    }

    private static boolean isJdbcPasswordMissing() {
        if (!"jdbc".equals(backend)) {
            return false;
        }
        return password == null || password.isBlank();
    }

    private static void processRequests(BufferedReader reader, PrintWriter writer) throws Exception {
        while (RUNNING.get()) {
            String line = readRequestLine(reader);
            if (line == null) {
                break;
            }
            processRequestLine(line, writer);
        }
    }

    private static String readRequestLine(BufferedReader reader) throws IOException {
        return reader.readLine();
    }

    private static void processRequestLine(String line, PrintWriter writer) {
        try {
            @SuppressWarnings("unchecked")
            Map<String, Object> request = MAPPER.readValue(line, Map.class);
            Object id = request.get("id");
            String method = (String) request.get("method");
            dispatchRequest(writer, request, id, method);
        } catch (Exception e) {
            error("Error processing request: " + summarizeException(e));
            sendRequestError(line, writer, e);
        }
    }

    private static void sendRequestError(String line, PrintWriter writer, Exception cause) {
        try {
            @SuppressWarnings("unchecked")
            final var failedRequest = (Map<String, Object>) MAPPER.readValue(line, Map.class);
            final var id = failedRequest.get("id");
            final var method = String.valueOf(failedRequest.get("method"));
            sendResponse(writer, id, null, Map.of(
                    "code", "REQUEST_FAILED",
                    "message", method + " failed: " + summarizeException(cause)));
        } catch (Exception responseError) {
            error("Failed to send error response: " + summarizeException(responseError));
        }
    }

    private static void closeBackends() throws SQLException {
        closeJdbcBackend();
        closeMongoBackend();
    }

    private static void closeJdbcBackend() throws SQLException {
        if (connection != null && !connection.isClosed()) {
            connection.close();
        }
    }

    private static void closeMongoBackend() {
        if (mongoClient != null) {
            mongoClient.close();
        }
    }

    private static void configureArguments(String[] args) {
        backend = "jdbc";
        driverClass = null;
        databaseUrl = null;
        user = null;
        passwordStdin = false;
        verboseMode = false;
        for (int i = 0; i < args.length; i++) {
            String argument = args[i];
            if ("--password-stdin".equals(argument)) {
                passwordStdin = true;
            } else if ("--verbose".equals(argument)) {
                verboseMode = true;
            } else if (ARGUMENTS.containsKey(argument) && i + 1 < args.length) {
                ARGUMENTS.get(argument).set(args[++i]);
            }
        }
    }

    private static void dispatchRequest(PrintWriter writer, Map<String, Object> request,
                                        Object id, String method) throws Exception {
        if ("shutdown".equals(method)) {
            sendResponse(writer, id, "bye", null);
            RUNNING.set(false);
            return;
        }
        RequestHandler handler = REQUEST_HANDLERS.get(method);
        if (handler == null) {
            sendResponse(writer, id, null,
                    Map.of("code", "UNKNOWN_METHOD", "message", "Unknown method: " + method));
            return;
        }
        touchActivity();
        handler.handle(writer, id, request);
    }

    private static void connectBackend() throws Exception {
        if ("jdbc".equals(backend)) {
            Class.forName(driverClass);
            DriverManager.setLoginTimeout(3);
            log("Connecting JDBC: url=" + databaseUrl + " user=" + user + " driver=" + driverClass);
            connection = DriverManager.getConnection(databaseUrl, user, password);
            applyStatementTimeout();
            configureReadOnlyConnection();
            return;
        }
        if ("mongodb".equals(backend)) {
            String url = databaseUrl.replace("__SAFESELECT_PASSWORD__", URLEncoder.encode(password == null ? "" : password, java.nio.charset.StandardCharsets.UTF_8));
            log("Connecting MongoDB: url=" + databaseUrl + " user=" + user);
            mongoClient = MongoClients.create(url);
            return;
        }
        throw new IllegalArgumentException("Unsupported backend: " + backend);
    }

    private static void applyStatementTimeout() throws SQLException {
        if (statementTimeoutMs > 0 && connection != null && !connection.isClosed()) {
            try (Statement s = connection.createStatement()) {
                s.execute("SET statement_timeout = " + statementTimeoutMs);
                log("Statement timeout set to " + statementTimeoutMs + "ms");
            }
        }
    }

    private static void configureReadOnlyConnection() throws SQLException {
        connection.setReadOnly(true);
        connection.setAutoCommit(false);
        verifyReadOnlyTransaction();
        connection.rollback();
    }

    private static void verifyReadOnlyTransaction() throws SQLException {
        try (Statement statement = connection.createStatement();
             ResultSet result = statement.executeQuery("SHOW transaction_read_only")) {
            if (!result.next() || !"on".equalsIgnoreCase(result.getString(1))) {
                throw new SQLException("SafeSelect could not establish a read-only transaction");
            }
        }
    }

    private static void rollbackReadOnlyTransaction() {
        try {
            if (connection != null && !connection.getAutoCommit()) {
                connection.rollback();
            }
        } catch (SQLException rollbackError) {
            error("Failed to rollback read-only transaction: " + summarizeException(rollbackError));
            closeJdbcAfterSecurityFailure();
        }
    }

    private static void closeJdbcAfterSecurityFailure() {
        try {
            closeJdbcBackend();
        } catch (SQLException closeError) {
            error("Failed to close unsafe JDBC connection: " + summarizeException(closeError));
        } finally {
            connection = null;
        }
    }

    private static void touchActivity() {
        lastActivityMs.set(System.currentTimeMillis());
    }

    /**
     * Convert PostgreSQL-specific objects to Java standard types for JSON serialization.
     */
    private static Object convertPgObject(Object val) throws Exception {
        if (val == null) {
            return null;
        }
        return convertPgObjectNonNull(val);
    }

    private static Object convertPgObjectNonNull(Object val) throws Exception {
        
        // Handle Clob
        if (val instanceof java.sql.Clob) {
            return ((java.sql.Clob) val).getSubString(1, (int) ((java.sql.Clob) val).length());
        }
        
        String className = val.getClass().getName();
        
        // Handle PGobject (jsonb, hstore, etc.) via reflection
        if (className.startsWith("org.postgresql.util.PGobject")) {
            return convertPgDriverObject(val);
        }
        
        // Handle PgArray
        if (val instanceof java.sql.Array) {
            java.sql.Array array = (java.sql.Array) val;
            Object[] arrayData = (Object[]) array.getArray();
            List<Object> converted = new ArrayList<>();
            for (Object item : arrayData) {
                converted.add(convertPgObject(item));
            }
            return converted;
        }
        
        return convertLegacyValue(val);
    }

    private static Object convertPgDriverObject(Object value) throws Exception {
        try {
            String pgValue = (String) value.getClass().getMethod("getValue").invoke(value);
            String pgType = (String) value.getClass().getMethod("getType").invoke(value);
            return Set.of("jsonb", "json").contains(pgType)
                    ? MAPPER.readValue(pgValue, Object.class)
                    : pgValue;
        } catch (Exception e) {
            log("[CONVERT] Failed to convert PGobject: " + e.getMessage());
            return value.toString();
        }
    }

    private static Object convertLegacyValue(Object value) throws Exception {
        try {
            java.lang.reflect.Method getValue = value.getClass().getMethod("getValue");
            return convertPgObject(getValue.invoke(value));
        } catch (NoSuchMethodException | SecurityException e) {
            return value;
        }
    }

    private static void startIdleTimer(PrintWriter writer) {
        Thread timer = new Thread(() -> {
            while (RUNNING.get()) {
                try {
                    Thread.sleep(1000);
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    break;
                }
                long idle = System.currentTimeMillis() - lastActivityMs.get();
                if (idle >= idleTimeoutMs) {
                    try {
                        if (connection != null && !connection.isClosed()) {
                            connection.close();
                            connection = null;
                            Map<String, Object> notification = new LinkedHashMap<>();
                            notification.put("type", "idle_disconnect");
                            notification.put("idle_ms", idle);
                            String json = MAPPER.writeValueAsString(notification);
                            synchronized (writer) {
                                writer.println(json);
                                writer.flush();
                            }
                        }
            } catch (Exception e) {
                error("Idle disconnect error: " + e.getMessage());
            }
                }
            }
        });
        timer.setDaemon(true);
        timer.start();
    }

    private static void handleDisconnect(PrintWriter writer, Object id) throws Exception {
        if ("mongodb".equals(backend)) {
            if (mongoClient == null) {
                sendResponse(writer, id, Map.of("status", "already_disconnected"), null);
                return;
            }
            mongoClient.close();
            mongoClient = null;
            sendResponse(writer, id, Map.of("status", "disconnected"), null);
            return;
        }
        if (connection == null || connection.isClosed()) {
            sendResponse(writer, id, Map.of("status", "already_disconnected"), null);
            return;
        }
        connection.close();
        connection = null;
        sendResponse(writer, id, Map.of("status", "disconnected"), null);
    }

    private static void handleConnect(PrintWriter writer, Object id) throws Exception {
        if ("mongodb".equals(backend)) {
            handleMongoConnect(writer, id);
            return;
        }
        handleJdbcConnect(writer, id);
    }

    private static void handleMongoConnect(PrintWriter writer, Object id) throws Exception {
        if (mongoClient != null) {
            sendResponse(writer, id, Map.of("status", "already_connected"), null);
            return;
        }
        String url = databaseUrl.replace("__SAFESELECT_PASSWORD__", URLEncoder.encode(password == null ? "" : password, java.nio.charset.StandardCharsets.UTF_8));
        mongoClient = MongoClients.create(url);
        sendResponse(writer, id, Map.of("status", "connected"), null);
    }

    private static void handleJdbcConnect(PrintWriter writer, Object id) throws Exception {
        if (connection != null && !connection.isClosed()) {
            try {
                if (connection.isValid(2)) {
                    sendResponse(writer, id, Map.of("status", "already_connected"), null);
                    return;
                }
                error("Existing JDBC connection is not valid; reconnecting");
            } catch (SQLException e) {
                error("JDBC validation failed before reconnect: " + e.getMessage());
            }

            try {
                connection.close();
            } catch (SQLException e) {
                error("Error closing stale JDBC connection: " + e.getMessage());
            }
            connection = null;
        }
        connection = DriverManager.getConnection(databaseUrl, user, password);
        applyStatementTimeout();
        configureReadOnlyConnection();
        sendResponse(writer, id, Map.of("status", "connected"), null);
    }

    private static void ensureMongoConnected(PrintWriter writer, Object id) throws Exception {
        if (!"mongodb".equals(backend)) {
            sendResponse(writer, id, null,
                    Map.of("code", "UNSUPPORTED_BACKEND", "message", "Document operations require a document backend."));
            throw new IllegalStateException("Unsupported backend for document operation");
        }
        if (mongoClient == null) {
            sendResponse(writer, id, null,
                    Map.of("code", "NOT_CONNECTED", "message", "Database not connected. Use 'connect' first."));
            throw new IllegalStateException("MongoDB is not connected");
        }
    }

    private static void handleListDatabases(PrintWriter writer, Object id) throws Exception {
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        List<String> databases = new ArrayList<>();
        for (String name : mongoClient.listDatabaseNames()) {
            databases.add(name);
        }
        sendResponse(writer, id, databases, null);
    }

    private static void handleVerifyDocumentConnection(PrintWriter writer, Object id) throws Exception {
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        final var result = mongoClient
                .getDatabase("admin")
                .runCommand(new Document("ping", 1), ReadPreference.secondaryPreferred());
        sendBoundedResponse(writer, id, result);
    }

    @SuppressWarnings("unchecked")
    private static void handleListCollections(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        if (params == null || params.get("database") == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_DATABASE", "message", "No database provided"));
            return;
        }
        MongoDatabase database = mongoClient.getDatabase((String) params.get("database"));
        List<String> collections = new ArrayList<>();
        for (String name : database.listCollectionNames()) {
            collections.add(name);
        }
        sendResponse(writer, id, collections, null);
    }

    @SuppressWarnings("unchecked")
    private static void handleListCollectionIndexes(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        if (params == null || params.get("database") == null || params.get("collection") == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_NAMESPACE", "message", "Database and collection are required"));
            return;
        }
        String databaseName = (String) params.get("database");
        String collectionName = (String) params.get("collection");
        MongoCollection<Document> collection = mongoClient.getDatabase(databaseName).getCollection(collectionName);

        List<Object> classicIndexes = new ArrayList<>();
        if (!appendClassicIndexes(writer, id, collection, classicIndexes)) return;

        List<Object> searchIndexes = new ArrayList<>();
        String searchStatus = appendSearchIndexes(writer, id, collection, searchIndexes);
        if (searchStatus == null) return;

        Map<String, Object> result = new LinkedHashMap<>();
        result.put("database", databaseName);
        result.put("collection", collectionName);
        result.put("classic_indexes", classicIndexes);
        result.put("search_indexes", searchIndexes);
        result.put("search_indexes_status", searchStatus);
        sendBoundedResponse(writer, id, result);
    }

    private static boolean appendClassicIndexes(
            PrintWriter writer,
            Object id,
            MongoCollection<Document> collection,
            List<Object> indexes) throws Exception {
        try (MongoCursor<Document> cursor = collection.listIndexes().iterator()) {
            while (cursor.hasNext()) {
                Document index = cursor.next();
                Map<String, Object> safeIndex = new LinkedHashMap<>();
                String indexName = index.getString("name");
                safeIndex.put("name", indexName);
                safeIndex.put("key", convertBsonValue(index.get("key")));
                safeIndex.put("unique", isClassicIndexUnique(indexName, index.getBoolean("unique", false)));
                safeIndex.put("sparse", index.getBoolean("sparse", false));
                safeIndex.put("partial_filter_expression", convertBsonValue(index.get("partialFilterExpression")));
                if (!appendBounded(indexes, safeIndex)) {
                    sendLimitExceeded(writer, id, "max_result_bytes", maxResultBytes);
                    return false;
                }
            }
        }
        return true;
    }

    private static String appendSearchIndexes(
            PrintWriter writer,
            Object id,
            MongoCollection<Document> collection,
            List<Object> indexes) throws Exception {
        try (MongoCursor<Document> cursor = collection.listSearchIndexes().iterator()) {
            while (cursor.hasNext()) {
                Document index = cursor.next();
                Map<String, Object> safeIndex = new LinkedHashMap<>();
                safeIndex.put("name", index.getString("name"));
                safeIndex.put("type", searchIndexType(index.getString("type")));
                safeIndex.put("status", index.getString("status"));
                safeIndex.put("queryable", index.getBoolean("queryable", false));
                safeIndex.put("definition", convertBsonValue(index.get("latestDefinition")));
                if (!appendBounded(indexes, safeIndex)) {
                    sendLimitExceeded(writer, id, "max_result_bytes", maxResultBytes);
                    return null;
                }
            }
        } catch (MongoCommandException e) {
            if (isSearchUnsupported(e)) return "unsupported";
            if (isSearchUnauthorized(e)) return "unauthorized";
            throw e;
        }
        return "available";
    }

    @SuppressWarnings("unchecked")
    private static void handleGetDatabaseStats(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        if (params == null || params.get("database") == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_DATABASE", "message", "No database provided"));
            return;
        }
        String databaseName = (String) params.get("database");
        Document command = new Document("dbStats", 1);
        applyMongoTimeout(command);
        Document stats = mongoClient.getDatabase(databaseName).runCommand(command);
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("database", databaseName);
        result.put("collections", stats.get("collections"));
        result.put("views", stats.get("views"));
        result.put("objects", stats.get("objects"));
        result.put("average_object_size", stats.get("avgObjSize"));
        result.put("data_size", stats.get("dataSize"));
        result.put("storage_size", stats.get("storageSize"));
        result.put("index_count", stats.get("indexes"));
        result.put("index_size", stats.get("indexSize"));
        result.put("total_size", stats.get("totalSize"));
        sendBoundedResponse(writer, id, result);
    }

    @SuppressWarnings("unchecked")
    private static void handleGetCollectionStats(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        if (params == null || params.get("database") == null || params.get("collection") == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_NAMESPACE", "message", "Database and collection are required"));
            return;
        }
        String databaseName = (String) params.get("database");
        String collectionName = (String) params.get("collection");
        Document command = new Document("collStats", collectionName);
        applyMongoTimeout(command);
        Document stats = mongoClient.getDatabase(databaseName).runCommand(command);
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("database", databaseName);
        result.put("collection", collectionName);
        result.put("document_count", stats.get("count"));
        result.put("average_object_size", stats.get("avgObjSize"));
        result.put("data_size", stats.get("size"));
        result.put("storage_size", stats.get("storageSize"));
        result.put("index_count", stats.get("nindexes"));
        result.put("total_index_size", stats.get("totalIndexSize"));
        sendBoundedResponse(writer, id, result);
    }

    private static boolean appendBounded(List<Object> values, Object value) throws Exception {
        long currentBytes = MAPPER.writeValueAsBytes(values).length;
        long valueBytes = MAPPER.writeValueAsBytes(value).length;
        if (currentBytes + valueBytes > maxResultBytes) {
            return false;
        }
        values.add(value);
        return true;
    }

    private static void sendBoundedResponse(PrintWriter writer, Object id, Object result) throws Exception {
        if (MAPPER.writeValueAsBytes(result).length > maxResultBytes) {
            sendLimitExceeded(writer, id, "max_result_bytes", maxResultBytes);
            return;
        }
        sendResponse(writer, id, result, null);
    }

    static String searchIndexType(String type) {
        return switch (type == null ? "" : type) {
            case "search", "vectorSearch", "autoEmbed" -> type;
            default -> "unknown";
        };
    }

    static boolean isClassicIndexUnique(String name, boolean explicitUnique) {
        return "_id_".equals(name) || explicitUnique;
    }

    static boolean isSearchUnsupported(MongoCommandException error) {
        return isSearchUnsupported(error.getErrorCode(), error.getErrorMessage());
    }

    static boolean isSearchUnsupported(int errorCode, String errorMessage) {
        String message = errorMessage == null ? "" : errorMessage.toLowerCase(Locale.ROOT);
        return errorCode == 59
                || errorCode == 31082 // SearchNotEnabled on local MongoDB deployments
                || message.contains("command not found")
                || message.contains("searchnotenabled");
    }

    static boolean isSearchUnauthorized(MongoCommandException error) {
        return error.getErrorCode() == 13 || error.getErrorMessage().toLowerCase(Locale.ROOT).contains("not authorized");
    }

    @SuppressWarnings("unchecked")
    private static void handleFindDocuments(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        long startTime = System.currentTimeMillis();
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        if (params == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_PARAMS", "message", "No params"));
            return;
        }
        String databaseName = (String) params.get("database");
        String collectionName = (String) params.get("collection");
        if (databaseName == null || collectionName == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_NAMESPACE", "message", "Database and collection are required"));
            return;
        }

        if (rejectMongoJavaScript(writer, id, params.get("filter"), params.get("projection"), params.get("sort"))) return;

        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        MongoCollection<Document> collection = mongoClient.getDatabase(databaseName).getCollection(collectionName);
        FindIterable<Document> find = collection.find(filter);
        applyMongoTimeout(find);

        applyFindModifiers(find, params);
        long requestedLimit = ((Number) params.getOrDefault("limit", Math.min(maxRows, 100))).longValue();
        int effectiveLimit = (int) Math.min(requestedLimit, maxRows);
        find.limit(effectiveLimit);

        List<Object> documents = new ArrayList<>();
        long byteCount = collectFindDocuments(writer, id, find, documents);
        if (byteCount < 0) return;

        long elapsedMs = System.currentTimeMillis() - startTime;
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("documents", documents);
        result.put("document_count", documents.size());
        result.put("byte_count", byteCount);
        result.put("elapsed_ms", elapsedMs);
        result.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, result, null);
    }

    private static void applyFindModifiers(FindIterable<Document> find, Map<String, Object> params) throws Exception {
        if (params.get("projection") != null) find.projection(toDocument(params.get("projection")));
        if (params.get("sort") != null) find.sort(toDocument(params.get("sort")));
    }

    private static long collectFindDocuments(
            PrintWriter writer,
            Object id,
            FindIterable<Document> find,
            List<Object> documents) throws Exception {
        long byteCount = 0;
        try (MongoCursor<Document> cursor = find.iterator()) {
            while (cursor.hasNext()) {
                Object converted = MAPPER.readValue(cursor.next().toJson(), Object.class);
                long documentBytes = MAPPER.writeValueAsBytes(converted).length;
                if (documents.size() >= maxRows) {
                    sendResponse(writer, id, null, Map.of(
                            "code", "RESULT_LIMIT_EXCEEDED",
                            "message", "Document limit exceeded: " + maxRows,
                            "limit_type", "max_rows",
                            "limit_value", maxRows));
                    return -1;
                }
                if (byteCount + documentBytes > maxResultBytes) {
                    sendResponse(writer, id, null, Map.of(
                            "code", "RESULT_LIMIT_EXCEEDED",
                            "message", "Result size limit exceeded: " + maxResultBytes + " bytes",
                            "limit_type", "max_result_bytes",
                            "limit_value", maxResultBytes));
                    return -1;
                }
                byteCount += documentBytes;
                documents.add(converted);
            }
        }
        return byteCount;
    }

    @SuppressWarnings("unchecked")
    private static void handleAggregateDocuments(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        long startTime = System.currentTimeMillis();
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        String databaseName = stringParam(params, "database");
        String collectionName = stringParam(params, "collection");
        if (databaseName == null || collectionName == null || !(params.get("pipeline") instanceof List<?> rawPipeline)) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_PARAMS", "message", "Database, collection and pipeline are required"));
            return;
        }
        if (rejectMongoJavaScript(writer, id, rawPipeline)) return;

        long requestedLimit = numberParam(params, "limit", Math.min(maxRows, 100));
        int effectiveLimit = (int) Math.min(requestedLimit, maxRows);
        List<Document> pipeline = buildReadOnlyPipeline(writer, id, rawPipeline, effectiveLimit);
        if (pipeline == null) return;

        MongoCollection<Document> collection = mongoClient.getDatabase(databaseName).getCollection(collectionName);
        AggregateIterable<Document> aggregate = collection.aggregate(pipeline).allowDiskUse(false);
        applyMongoTimeout(aggregate);
        sendDocumentIterable(writer, id, aggregate.iterator(), startTime, "documents", "document_count");
    }

    private static List<Document> buildReadOnlyPipeline(
            PrintWriter writer,
            Object id,
            List<?> rawPipeline,
            int effectiveLimit) throws Exception {
        List<Document> pipeline = new ArrayList<>();
        for (Object stage : rawPipeline) {
            Document document = toDocument(stage);
            for (String key : document.keySet()) {
                if ("$out".equals(key) || "$merge".equals(key) || "$currentOp".equals(key)) {
                    sendResponse(writer, id, null, Map.of(
                            "code", "NOT_READ_ONLY",
                            "message", "Aggregation stage is not read-only: " + key));
                    return null;
                }
            }
            pipeline.add(document);
        }
        pipeline.add(new Document("$limit", effectiveLimit));
        return pipeline;
    }

    @SuppressWarnings("unchecked")
    private static void handleDistinctDocuments(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        long startTime = System.currentTimeMillis();
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        String databaseName = stringParam(params, "database");
        String collectionName = stringParam(params, "collection");
        String field = stringParam(params, "field");
        if (databaseName == null || collectionName == null || field == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_PARAMS", "message", "Database, collection and field are required"));
            return;
        }
        if (rejectMongoJavaScript(writer, id, params.get("filter"))) return;
        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        long limit = numberParam(params, "limit", Math.min(maxRows, 100));
        MongoCollection<Document> collection = mongoClient.getDatabase(databaseName).getCollection(collectionName);
        List<Object> values = new ArrayList<>();
        var distinct = collection.distinct(field, filter, BsonValue.class);
        applyMongoTimeout(distinct);
        long byteCount = collectDistinctValues(writer, id, distinct, limit, values);
        if (byteCount < 0) return;
        long elapsedMs = System.currentTimeMillis() - startTime;
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("values", values);
        result.put("value_count", values.size());
        result.put("byte_count", byteCount);
        result.put("elapsed_ms", elapsedMs);
        result.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, result, null);
    }

    private static long collectDistinctValues(
            PrintWriter writer,
            Object id,
            com.mongodb.client.DistinctIterable<BsonValue> distinct,
            long limit,
            List<Object> values) throws Exception {
        long byteCount = 0;
        try (MongoCursor<BsonValue> cursor = distinct.iterator()) {
            while (cursor.hasNext() && values.size() < limit && values.size() < maxRows) {
                Object value = convertBsonValue(cursor.next());
                long valueBytes = MAPPER.writeValueAsBytes(value).length;
                if (byteCount + valueBytes > maxResultBytes) {
                    sendLimitExceeded(writer, id, "max_result_bytes", maxResultBytes);
                    return -1;
                }
                byteCount += valueBytes;
                values.add(value);
            }
        }
        return byteCount;
    }

    @SuppressWarnings("unchecked")
    private static void handleCountDocuments(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        long startTime = System.currentTimeMillis();
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        String databaseName = stringParam(params, "database");
        String collectionName = stringParam(params, "collection");
        if (databaseName == null || collectionName == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_PARAMS", "message", "Database and collection are required"));
            return;
        }
        if (rejectMongoJavaScript(writer, id, params.get("filter"))) return;
        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        var countOptions = new com.mongodb.client.model.CountOptions();
        applyMongoTimeout(countOptions);
        long count = mongoClient.getDatabase(databaseName).getCollection(collectionName).countDocuments(filter, countOptions);
        long elapsedMs = System.currentTimeMillis() - startTime;
        sendResponse(writer, id, Map.of("count", count, "elapsed_ms", elapsedMs, "elapsed", formatElapsed(elapsedMs)), null);
    }

    @SuppressWarnings("unchecked")
    private static void handleExplainDocuments(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        long startTime = System.currentTimeMillis();
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        String databaseName = stringParam(params, "database");
        String collectionName = stringParam(params, "collection");
        if (databaseName == null || collectionName == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_PARAMS", "message", "Database and collection are required"));
            return;
        }
        if (rejectMongoJavaScript(writer, id, params.get("filter"), params.get("projection"), params.get("sort"))) return;
        Document explain = buildExplainCommand(collectionName, params);
        Document result = mongoClient.getDatabase(databaseName).runCommand(explain, ReadPreference.secondaryPreferred());
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("explain", convertBsonValue(result));
        long elapsedMs = System.currentTimeMillis() - startTime;
        response.put("elapsed_ms", elapsedMs);
        response.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, response, null);
    }

    private static Document buildExplainCommand(String collectionName, Map<String, Object> params) throws Exception {
        Document find = new Document("find", collectionName)
                .append("filter", toDocument(params.getOrDefault("filter", Map.of())));
        if (params.get("projection") != null) find.append("projection", toDocument(params.get("projection")));
        if (params.get("sort") != null) find.append("sort", toDocument(params.get("sort")));
        if (params.get("limit") != null) {
            find.append("limit", numberParam(params, "limit", Math.min(maxRows, 100)));
        }
        applyMongoTimeout(find);
        Document explain = new Document("explain", find);
        if (statementTimeoutMs > 0) explain.append("maxTimeMS", statementTimeoutMs);
        return explain;
    }

    @SuppressWarnings("unchecked")
    private static void handleProfileDocumentField(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        long startTime = System.currentTimeMillis();
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        String databaseName = stringParam(params, "database");
        String collectionName = stringParam(params, "collection");
        String field = stringParam(params, "field");
        if (databaseName == null || collectionName == null || field == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_PARAMS", "message", "Database, collection and field are required"));
            return;
        }
        if (rejectMongoJavaScript(writer, id, params.get("filter"))) return;
        long sampleSize = numberParam(params, "sample_size", Math.min(maxRows, 1000));
        long exampleLimit = numberParam(params, "examples", 5);
        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        MongoCollection<Document> collection = mongoClient.getDatabase(databaseName).getCollection(collectionName);
        FieldStats stats = new FieldStats(exampleLimit);
        long scanned = 0;
        FindIterable<Document> find = collection.find(filter).limit((int) Math.min(sampleSize, maxRows));
        applyMongoTimeout(find);
        try (MongoCursor<Document> cursor = find.iterator()) {
            while (cursor.hasNext()) {
                scanned++;
                stats.accept(resolvePath(cursor.next(), field));
            }
        }
        Map<String, Object> result = stats.toMap();
        result.put("field", field);
        result.put("sampled_documents", scanned);
        long elapsedMs = System.currentTimeMillis() - startTime;
        result.put("elapsed_ms", elapsedMs);
        result.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, result, null);
    }

    @SuppressWarnings("unchecked")
    private static void handleDiscoverDocumentSchema(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        long startTime = System.currentTimeMillis();
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        String databaseName = stringParam(params, "database");
        String collectionName = stringParam(params, "collection");
        if (databaseName == null || collectionName == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_PARAMS", "message", "Database and collection are required"));
            return;
        }
        if (rejectMongoJavaScript(writer, id, params.get("filter"))) return;
        long sampleSize = numberParam(params, "sample_size", Math.min(maxRows, 1000));
        long exampleLimit = numberParam(params, "examples", 3);
        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        MongoCollection<Document> collection = mongoClient.getDatabase(databaseName).getCollection(collectionName);
        Map<String, FieldStats> fields = new TreeMap<>();
        long scanned = 0;
        FindIterable<Document> find = collection.find(filter).limit((int) Math.min(sampleSize, maxRows));
        applyMongoTimeout(find);
        try (MongoCursor<Document> cursor = find.iterator()) {
            while (cursor.hasNext()) {
                scanned++;
                collectFields("", cursor.next(), fields, exampleLimit);
            }
        }
        List<Object> fieldSummaries = new ArrayList<>();
        for (Map.Entry<String, FieldStats> entry : fields.entrySet()) {
            Map<String, Object> summary = entry.getValue().toMap();
            summary.put("field", entry.getKey());
            fieldSummaries.add(summary);
        }
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("sampled_documents", scanned);
        result.put("fields", fieldSummaries);
        long elapsedMs = System.currentTimeMillis() - startTime;
        result.put("elapsed_ms", elapsedMs);
        result.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, result, null);
    }

    @SuppressWarnings("unchecked")
    private static void handleGenerateDocumentFixture(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        long startTime = System.currentTimeMillis();
        try {
            ensureMongoConnected(writer, id);
        } catch (IllegalStateException e) {
            return;
        }
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        String databaseName = stringParam(params, "database");
        String collectionName = stringParam(params, "collection");
        if (databaseName == null || collectionName == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_PARAMS", "message", "Database and collection are required"));
            return;
        }
        if (rejectMongoJavaScript(writer, id, params.get("filter"), params.get("projection"))) return;
        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        long limit = numberParam(params, "limit", Math.min(maxRows, 20));
        FindIterable<Document> find = mongoClient.getDatabase(databaseName).getCollection(collectionName)
                .find(filter)
                .limit((int) Math.min(limit, maxRows));
        applyMongoTimeout(find);
        if (params.get("projection") != null) {
            find.projection(toDocument(params.get("projection")));
        }
        List<String> redactFields = fixtureRedactFields(params);
        List<Object> documents = new ArrayList<>();
        long byteCount = collectFixtureDocuments(writer, id, find, redactFields, documents);
        if (byteCount < 0) return;
        long elapsedMs = System.currentTimeMillis() - startTime;
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("documents", documents);
        result.put("document_count", documents.size());
        result.put("byte_count", byteCount);
        result.put("redacted_fields", redactFields);
        result.put("elapsed_ms", elapsedMs);
        result.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, result, null);
    }

    private static List<String> fixtureRedactFields(Map<String, Object> params) {
        if (!(params.get("redact_fields") instanceof List<?> fields)) return new ArrayList<>();
        return fields.stream()
                .filter(String.class::isInstance)
                .map(String.class::cast)
                .toList();
    }

    private static long collectFixtureDocuments(
            PrintWriter writer,
            Object id,
            FindIterable<Document> find,
            List<String> redactFields,
            List<Object> documents) throws Exception {
        long byteCount = 0;
        try (MongoCursor<Document> cursor = find.iterator()) {
            while (cursor.hasNext()) {
                Object converted = convertBsonValue(cursor.next());
                redactValue(converted, "", redactFields);
                long documentBytes = MAPPER.writeValueAsBytes(converted).length;
                if (byteCount + documentBytes > maxResultBytes) {
                    sendLimitExceeded(writer, id, "max_result_bytes", maxResultBytes);
                    return -1;
                }
                byteCount += documentBytes;
                documents.add(converted);
            }
        }
        return byteCount;
    }

    private static Document toDocument(Object value) throws Exception {
        if (value == null) {
            return new Document();
        }
        if (value instanceof Document document) {
            return document;
        }
        return Document.parse(MAPPER.writeValueAsString(value));
    }

    static String forbiddenMongoJavaScriptOperator(Object value) {
        if (value instanceof Map<?, ?> values) {
            return forbiddenMongoJavaScriptMap(values);
        }
        if (value instanceof Iterable<?> values) {
            return forbiddenMongoJavaScriptIterable(values);
        }
        return null;
    }

    private static String forbiddenMongoJavaScriptMap(Map<?, ?> values) {
        for (Map.Entry<?, ?> entry : values.entrySet()) {
            String key = String.valueOf(entry.getKey());
            if ("$where".equals(key) || "$function".equals(key) || "$accumulator".equals(key)) {
                return key;
            }
            String nested = forbiddenMongoJavaScriptOperator(entry.getValue());
            if (nested != null) return nested;
        }
        return null;
    }

    private static String forbiddenMongoJavaScriptIterable(Iterable<?> values) {
        for (Object entry : values) {
            String nested = forbiddenMongoJavaScriptOperator(entry);
            if (nested != null) return nested;
        }
        return null;
    }

    private static boolean rejectMongoJavaScript(PrintWriter writer, Object id, Object... values) throws Exception {
        for (Object value : values) {
            if (forbiddenMongoJavaScriptOperator(value) != null) {
                sendResponse(writer, id, null, Map.of(
                        "code", "JAVASCRIPT_DISABLED",
                        "message", "MongoDB server-side JavaScript is not allowed; rebuild the query using declarative MQL operators"
                ));
                return true;
            }
        }
        return false;
    }

    private static String stringParam(Map<String, Object> params, String name) {
        if (params == null) {
            return null;
        }
        Object value = params.get(name);
        return value instanceof String stringValue ? stringValue : null;
    }

    private static long numberParam(Map<String, Object> params, String name, long defaultValue) {
        if (params == null) {
            return defaultValue;
        }
        Object value = params.get(name);
        return value instanceof Number numberValue ? numberValue.longValue() : defaultValue;
    }

    private static void applyMongoTimeout(FindIterable<Document> iterable) {
        if (statementTimeoutMs > 0) {
            iterable.maxTime(statementTimeoutMs, TimeUnit.MILLISECONDS);
        }
    }

    private static void applyMongoTimeout(AggregateIterable<Document> iterable) {
        if (statementTimeoutMs > 0) {
            iterable.maxTime(statementTimeoutMs, TimeUnit.MILLISECONDS);
        }
    }

    private static void applyMongoTimeout(com.mongodb.client.DistinctIterable<BsonValue> iterable) {
        if (statementTimeoutMs > 0) {
            iterable.maxTime(statementTimeoutMs, TimeUnit.MILLISECONDS);
        }
    }

    private static void applyMongoTimeout(Document command) {
        if (statementTimeoutMs > 0) {
            command.append("maxTimeMS", statementTimeoutMs);
        }
    }

    private static void applyMongoTimeout(com.mongodb.client.model.CountOptions options) {
        if (statementTimeoutMs > 0) {
            options.maxTime(statementTimeoutMs, TimeUnit.MILLISECONDS);
        }
    }

    private static Object convertBsonValue(Object value) throws Exception {
        if (value instanceof BsonValue bsonValue) {
            return convertBsonValue(bsonValue);
        }
        if (value instanceof Document document) {
            return MAPPER.readValue(document.toJson(), Object.class);
        }
        return MAPPER.readValue(MAPPER.writeValueAsString(value), Object.class);
    }

    private static Object convertBsonValue(BsonValue value) {
        if (value == null || value.isNull()) {
            return null;
        }
        if (value.getBsonType() == BsonType.UNDEFINED) {
            return null;
        }
        if (SCALAR_BSON_TYPES.contains(value.getBsonType())) {
            return convertBsonScalar(value);
        }
        return convertBsonComposite(value);
    }

    private static Object convertBsonScalar(BsonValue value) {
        return BSON_SCALAR_CONVERTERS.get(value.getBsonType()).apply(value);
    }

    private static Object convertBsonComposite(BsonValue value) {
        if (value.isRegularExpression()) {
            Map<String, Object> regex = new LinkedHashMap<>();
            regex.put("_bson_type", "regular_expression");
            regex.put("pattern", value.asRegularExpression().getPattern());
            regex.put("options", value.asRegularExpression().getOptions());
            return regex;
        }
        if (value.isBinary()) {
            BsonBinary binary = value.asBinary();
            Map<String, Object> result = new LinkedHashMap<>();
            result.put("_bson_type", "binary");
            result.put("type", binary.getType());
            result.put("base64", Base64.getEncoder().encodeToString(binary.getData()));
            return result;
        }
        if (value.isArray()) {
            BsonArray array = value.asArray();
            List<Object> result = new ArrayList<>();
            for (BsonValue item : array) {
                result.add(convertBsonValue(item));
            }
            return result;
        }
        if (value.isDocument()) {
            BsonDocument document = value.asDocument();
            Map<String, Object> result = new LinkedHashMap<>();
            for (Map.Entry<String, BsonValue> entry : document.entrySet()) {
                result.put(entry.getKey(), convertBsonValue(entry.getValue()));
            }
            return result;
        }
        return Map.of("_bson_type", value.getBsonType().name().toLowerCase(Locale.ROOT));
    }

    private static void sendDocumentIterable(
            PrintWriter writer,
            Object id,
            MongoCursor<Document> cursor,
            long startTime,
            String documentsKey,
            String countKey
    ) throws Exception {
        List<Object> documents = new ArrayList<>();
        long byteCount = 0;
        try (cursor) {
            while (cursor.hasNext()) {
                if (documents.size() >= maxRows) {
                    sendLimitExceeded(writer, id, "max_rows", maxRows);
                    return;
                }
                Object converted = convertBsonValue(cursor.next());
                long documentBytes = MAPPER.writeValueAsBytes(converted).length;
                if (byteCount + documentBytes > maxResultBytes) {
                    sendLimitExceeded(writer, id, "max_result_bytes", maxResultBytes);
                    return;
                }
                byteCount += documentBytes;
                documents.add(converted);
            }
        }
        long elapsedMs = System.currentTimeMillis() - startTime;
        Map<String, Object> result = new LinkedHashMap<>();
        result.put(documentsKey, documents);
        result.put(countKey, documents.size());
        result.put("byte_count", byteCount);
        result.put("elapsed_ms", elapsedMs);
        result.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, result, null);
    }

    private static void sendLimitExceeded(PrintWriter writer, Object id, String limitType, long limitValue) throws Exception {
        sendResponse(writer, id, null, Map.of(
                "code", "RESULT_LIMIT_EXCEEDED",
                "message", "Result limit exceeded: " + limitValue,
                "limit_type", limitType,
                "limit_value", limitValue
        ));
    }

    @SuppressWarnings("unchecked")
    private static Object resolvePath(Object value, String path) {
        return resolvePath(value, path.split("\\."), 0);
    }

    private static Object resolvePath(Object value, String[] parts, int index) {
        if (index >= parts.length) {
            return value;
        }
        return resolvePathContainer(value, parts, index);
    }

    private static Object resolvePathContainer(Object value, String[] parts, int index) {
        if (isMapLike(value)) {
            return resolveMapLikePath(value, parts, index);
        }
        if (value instanceof List<?> list) {
            return resolveListPath(list, parts, index);
        }
        return MissingValue.INSTANCE;
    }

    private static boolean isMapLike(Object value) {
        return value instanceof Map<?, ?> || value instanceof Document;
    }

    private static Object resolveMapLikePath(Object value, String[] parts, int index) {
        if (value instanceof Document document) {
            return resolveDocumentPath(document, parts, index);
        }
        return resolveMapPath((Map<?, ?>) value, parts, index);
    }

    private static Object resolveMapPath(Map<?, ?> map, String[] parts, int index) {
        return map.containsKey(parts[index])
                ? resolvePath(map.get(parts[index]), parts, index + 1)
                : MissingValue.INSTANCE;
    }

    private static Object resolveDocumentPath(Document document, String[] parts, int index) {
        return document.containsKey(parts[index])
                ? resolvePath(document.get(parts[index]), parts, index + 1)
                : MissingValue.INSTANCE;
    }

    private static Object resolveListPath(List<?> list, String[] parts, int index) {
        List<Object> values = list.stream()
                .map(item -> resolvePath(item, parts, index))
                .filter(nested -> nested != MissingValue.INSTANCE)
                .toList();
        return values.isEmpty() ? MissingValue.INSTANCE : values;
    }

    @SuppressWarnings("unchecked")
    private static void collectFields(String prefix, Object value, Map<String, FieldStats> fields, long exampleLimit) throws Exception {
        Object converted = value instanceof Document ? convertBsonValue(value) : value;
        if (converted instanceof Map<?, ?> map) {
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                String key = String.valueOf(entry.getKey());
                String path = prefix.isEmpty() ? key : prefix + "." + key;
                Object child = entry.getValue();
                fields.computeIfAbsent(path, ignored -> new FieldStats(exampleLimit)).accept(child);
                collectFields(path, child, fields, exampleLimit);
            }
            return;
        }
        if (converted instanceof List<?> list) {
            for (Object item : list) {
                collectFields(prefix, item, fields, exampleLimit);
            }
        }
    }

    @SuppressWarnings("unchecked")
    private static void redactValue(Object value, String prefix, List<String> redactFields) {
        if (value instanceof Map<?, ?> rawMap) {
            Map<String, Object> map = (Map<String, Object>) rawMap;
            for (Map.Entry<String, Object> entry : map.entrySet()) {
                String path = prefix.isEmpty() ? entry.getKey() : prefix + "." + entry.getKey();
                if (redactFields.contains(path) || redactFields.contains(entry.getKey())) {
                    entry.setValue("[REDACTED]");
                } else {
                    redactValue(entry.getValue(), path, redactFields);
                }
            }
            return;
        }
        if (value instanceof List<?> list) {
            for (Object item : list) {
                redactValue(item, prefix, redactFields);
            }
        }
    }

    private enum MissingValue {
        INSTANCE
    }

    private static final class FieldStats {
        private final long exampleLimit;
        private long present;
        private long missing;
        private long nulls;
        private final Map<String, Long> types = new TreeMap<>();
        private final Set<Object> examples = new LinkedHashSet<>();
        private long arrayCount;
        private long arrayItems;
        private long minArraySize = Long.MAX_VALUE;
        private long maxArraySize;

        FieldStats(long exampleLimit) {
            this.exampleLimit = exampleLimit;
        }

        void accept(Object value) throws Exception {
            if (value == MissingValue.INSTANCE) {
                this.missing++;
                return;
            }
            this.present++;
            if (value == null) {
                this.nulls++;
                this.types.merge("null", 1L, Long::sum);
                return;
            }
            Object converted = value instanceof Document ? convertBsonValue(value) : value;
            this.types.merge(typeName(converted), 1L, Long::sum);
            if (converted instanceof List<?> list) {
                this.arrayCount++;
                this.arrayItems += list.size();
                this.minArraySize = Math.min(this.minArraySize, list.size());
                this.maxArraySize = Math.max(this.maxArraySize, list.size());
            }
            if (this.examples.size() < this.exampleLimit) {
                this.examples.add(converted);
            }
        }

        Map<String, Object> toMap() {
            Map<String, Object> result = new LinkedHashMap<>();
            result.put("present", this.present);
            result.put("missing", this.missing);
            result.put("nulls", this.nulls);
            result.put("types", this.types);
            result.put("approx_cardinality", this.examples.size());
            result.put("examples", new ArrayList<>(this.examples));
            if (this.arrayCount > 0) {
                result.put("array_count", this.arrayCount);
                result.put("min_array_size", this.minArraySize);
                result.put("max_array_size", this.maxArraySize);
                result.put("avg_array_size", this.arrayItems / (double) this.arrayCount);
            }
            return result;
        }

        private static String typeName(Object value) {
            if (value instanceof List<?>) {
                return "array";
            }
            if (value instanceof Map<?, ?>) {
                return "object";
            }
            if (value instanceof String) {
                return "string";
            }
            if (value instanceof Number) {
                return "number";
            }
            if (value instanceof Boolean) {
                return "boolean";
            }
            return value.getClass().getSimpleName();
        }
    }

    private static void handleExecute(PrintWriter writer, Object id, Map<String, Object> request) throws Exception { long startTime = System.currentTimeMillis();
        log("[EXECUTE] Starting query execution, id=" + id);
        
        if (!ensureConnected(writer, id)) {
            return;
        }

        String sql = validatedSql(writer, id, request);
        if (sql == null) {
            return;
        }

        log("[EXECUTE] SQL: " + sql.substring(0, Math.min(100, sql.length())) + "...");

        try {
            verifyReadOnlyTransaction();
        } catch (SQLException e) {
            closeJdbcAfterSecurityFailure();
            sendSqlError(writer, id, e);
            return;
        }

        try (Statement stmt = connection.createStatement()) {
            configureStatementTimeout(stmt);
            log("[EXECUTE] Executing statement...");
            boolean isResultSet = stmt.execute(sql);
            log("[EXECUTE] Statement executed in " + (System.currentTimeMillis() - startTime) + "ms, isResultSet=" + isResultSet);

            if (isResultSet) {
                try (ResultSet rs = stmt.getResultSet()) {
                    writeResultSetResponse(writer, id, rs, startTime);
                }
            } else {
                writeStatementResponse(writer, id, startTime);
            }
        } catch (SQLException e) {
            sendSqlError(writer, id, e);
        } finally {
            rollbackReadOnlyTransaction();
        }
    }

    private static boolean ensureConnected(PrintWriter writer, Object id) throws Exception {
        if (connection == null || connection.isClosed()) {
            error("Not connected, returning error");
            sendResponse(writer, id, null,
                    Map.of("code", "NOT_CONNECTED", "message", "Database not connected. Use 'connect' first."));
            return false;
        }
        return true;
    }

    @SuppressWarnings("unchecked")
    private static String validatedSql(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        if (params == null) {
            error("Missing params");
            sendResponse(writer, id, null, Map.of("code", "MISSING_PARAMS", "message", "No params"));
            return null;
        }

        String sql = (String) params.get("sql");
        if (sql == null || sql.isBlank()) {
            error("Missing SQL");
            sendResponse(writer, id, null, Map.of("code", "MISSING_SQL", "message", "No SQL provided"));
            return null;
        }
        return sql;
    }

    private static void configureStatementTimeout(Statement stmt) throws SQLException {
        if (statementTimeoutMs > 0) {
            int timeoutSeconds = (int) Math.ceil(statementTimeoutMs / 1000.0);
            stmt.setQueryTimeout(timeoutSeconds);
        }
    }

    private static void sendSqlError(PrintWriter writer, Object id, SQLException e) throws Exception {
            error("SQL error: " + e.getMessage() + " (state=" + e.getSQLState() + ")");
            Map<String, Object> error = new LinkedHashMap<>();
            error.put("code", "SQL_ERROR");
            error.put("sql_state", e.getSQLState());
            error.put("error_code", e.getErrorCode());
            String sqlState = e.getSQLState();
            if ("57014".equals(sqlState) && statementTimeoutMs > 0) {
                error.put("message", "Statement timeout exceeded: " + statementTimeoutMs + "ms - the query took too long to execute");
                error.put("timeout_ms", statementTimeoutMs);
            } else {
                error.put("message", e.getMessage());
            }
            sendResponse(writer, id, null, error);
    }

    private static void writeResultSetResponse(PrintWriter writer, Object id, ResultSet rs, long startTime) throws Exception { ResultSetMetaData meta = rs.getMetaData();
        int columnCount = meta.getColumnCount();
        List<String> columns = new ArrayList<>();
        for (int i = 1; i <= columnCount; i++) {
            columns.add(meta.getColumnName(i));
        }

        List<List<Object>> rows = new ArrayList<>();
        long rowCount = 0;
        long byteCount = 0;
        log("[EXECUTE] Reading result set...");
        while (rs.next()) {
            if (rowCount >= maxRows) {
                sendResponse(writer, id, null, Map.of("code", "RESULT_LIMIT_EXCEEDED",
                        "message", "Row limit exceeded: " + maxRows,
                        "limit_type", "max_rows", "limit_value", maxRows));
                return;
            }
            ResultRow resultRow = readResultRow(rs, columnCount);
            if (byteCount + resultRow.bytes() > maxResultBytes) {
                sendResponse(writer, id, null, Map.of("code", "RESULT_LIMIT_EXCEEDED",
                        "message", "Result size limit exceeded: " + maxResultBytes + " bytes",
                        "limit_type", "max_result_bytes", "limit_value", maxResultBytes));
                return;
            }
            byteCount += resultRow.bytes();
            rows.add(resultRow.values());
            rowCount++;
        }
        long elapsedMs = System.currentTimeMillis() - startTime;
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("columns", columns);
        result.put("rows", rows);
        result.put("row_count", rowCount);
        result.put("byte_count", byteCount);
        result.put("elapsed_ms", elapsedMs);
        result.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, result, null);
        log("[EXECUTE] Completed in " + elapsedMs + "ms");
    }

    private record ResultRow(List<Object> values, long bytes) { }

    private static ResultRow readResultRow(ResultSet rs, int columnCount) throws Exception { List<Object> values = new ArrayList<>();
        long bytes = 0;
        for (int i = 1; i <= columnCount; i++) {
            Object value = convertPgObject(rs.getObject(i));
            values.add(value);
            if (value != null) {
                bytes += value.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8).length;
            }
        }
        return new ResultRow(values, bytes);
    }

    private static void writeStatementResponse(PrintWriter writer, Object id, long startTime)
            throws Exception {
        long elapsedMs = System.currentTimeMillis() - startTime;
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("elapsed_ms", elapsedMs);
        result.put("elapsed", formatElapsed(elapsedMs));
        log("[EXECUTE] Non-result statement completed in " + elapsedMs + "ms");
        sendResponse(writer, id, result, null);
    }

    private static void sendResponse(PrintWriter writer, Object id, Object ok, Object error) throws Exception {
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("id", id);
        if (ok != null) {
            response.put("ok", ok);
        }
        if (error != null) {
            response.put("error", error);
        }
        String json = MAPPER.writeValueAsString(response);
        writer.println(json);
        writer.flush();
    }

    private static String formatElapsed(long elapsedMs) {
        if (elapsedMs < 1000) {
            return elapsedMs + "ms";
        }
        if (elapsedMs < 60000) {
            return String.format(Locale.ROOT, "%.1fs", elapsedMs / 1000.0);
        }

        long totalSeconds = elapsedMs / 1000;
        long minutes = totalSeconds / 60;
        long seconds = totalSeconds % 60;
        if (seconds == 0) {
            return minutes + "m";
        }
        return minutes + "m " + seconds + "s";
    }
}
