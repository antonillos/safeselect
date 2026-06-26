package com.safeselect;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.mongodb.client.FindIterable;
import com.mongodb.client.AggregateIterable;
import com.mongodb.client.MongoClient;
import com.mongodb.client.MongoClients;
import com.mongodb.client.MongoCollection;
import com.mongodb.client.MongoCursor;
import com.mongodb.client.MongoDatabase;
import com.mongodb.ReadPreference;
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
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;

public class Main {
    private static final ObjectMapper MAPPER = new ObjectMapper();
    private static final AtomicBoolean RUNNING = new AtomicBoolean(true);
    private static final long MAX_LOG_BYTES = 10L * 1024 * 1024;
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
    private static final AtomicLong lastActivityMs = new AtomicLong(System.currentTimeMillis());
    private static PrintWriter logWriter;

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
        backend = "jdbc";
        driverClass = null;
        databaseUrl = null;
        user = null;
        boolean passwordStdin = false;

        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--backend" -> backend = args[++i];
                case "--driver" -> driverClass = args[++i];
                case "--url" -> databaseUrl = args[++i];
                case "--user" -> user = args[++i];
                case "--password-stdin" -> passwordStdin = true;
                case "--idle-timeout-seconds" -> idleTimeoutMs = Long.parseLong(args[++i]) * 1000;
                case "--statement-timeout-ms" -> statementTimeoutMs = Long.parseLong(args[++i]);
                case "--max-rows" -> maxRows = Long.parseLong(args[++i]);
                case "--max-result-bytes" -> maxResultBytes = Long.parseLong(args[++i]);
                case "--verbose" -> verboseMode = true;
            }
        }

        if (verboseMode) {
            initializeLogWriter();
            log("Starting sidecar");
        }

        if (databaseUrl == null || user == null || !passwordStdin || ("jdbc".equals(backend) && driverClass == null)) {
            error("Usage: --backend <jdbc|mongodb> [--driver <class>] --url <url> --user <name> --password-stdin [--idle-timeout-seconds <sec>] [--statement-timeout-ms <ms>] [--max-rows <n>] [--max-result-bytes <n>]");
            System.exit(1);
        }

        BufferedReader reader = new BufferedReader(new InputStreamReader(System.in));
        PrintWriter writer = new PrintWriter(new OutputStreamWriter(System.out));

        password = reader.readLine();
        if ("jdbc".equals(backend) && (password == null || password.isBlank())) {
            error("Password required on stdin");
            System.exit(1);
        }

        if (idleTimeoutMs > 0) {
            startIdleTimer(writer);
        }

        try {
            connectBackend();

            writer.println("ready");
            writer.flush();

            while (RUNNING.get()) {
                String line = reader.readLine();
                if (line == null) break;

                try {
                    @SuppressWarnings("unchecked")
                    Map<String, Object> request = MAPPER.readValue(line, Map.class);
                    Object id = request.get("id");
                    String method = (String) request.get("method");

                    switch (method) {
                        case "ping" -> {
                            touchActivity();
                            sendResponse(writer, id, "pong", null);
                        }
                        case "execute" -> {
                            touchActivity();
                            handleExecute(writer, id, request);
                        }
                        case "list_databases" -> {
                            touchActivity();
                            handleListDatabases(writer, id);
                        }
                        case "verify_document_connection" -> {
                            touchActivity();
                            handleVerifyDocumentConnection(writer, id);
                        }
                        case "list_collections" -> {
                            touchActivity();
                            handleListCollections(writer, id, request);
                        }
                        case "find_documents" -> {
                            touchActivity();
                            handleFindDocuments(writer, id, request);
                        }
                        case "aggregate_documents" -> {
                            touchActivity();
                            handleAggregateDocuments(writer, id, request);
                        }
                        case "distinct_documents" -> {
                            touchActivity();
                            handleDistinctDocuments(writer, id, request);
                        }
                        case "count_documents" -> {
                            touchActivity();
                            handleCountDocuments(writer, id, request);
                        }
                        case "explain_documents" -> {
                            touchActivity();
                            handleExplainDocuments(writer, id, request);
                        }
                        case "profile_document_field" -> {
                            touchActivity();
                            handleProfileDocumentField(writer, id, request);
                        }
                        case "discover_document_schema" -> {
                            touchActivity();
                            handleDiscoverDocumentSchema(writer, id, request);
                        }
                        case "generate_document_fixture" -> {
                            touchActivity();
                            handleGenerateDocumentFixture(writer, id, request);
                        }
                        case "disconnect" -> {
                            touchActivity();
                            handleDisconnect(writer, id);
                        }
                        case "connect" -> {
                            touchActivity();
                            handleConnect(writer, id);
                        }
                        case "shutdown" -> {
                            sendResponse(writer, id, "bye", null);
                            RUNNING.set(false);
                        }
                        default -> sendResponse(writer, id, null,
                                Map.of("code", "UNKNOWN_METHOD", "message", "Unknown method: " + method));
                    }
                } catch (Exception e) {
                    error("Error processing request: " + summarizeException(e));
                    try {
                        @SuppressWarnings("unchecked")
                        final var failedRequest = (Map<String, Object>) MAPPER.readValue(line, Map.class);
                        final var id = failedRequest.get("id");
                        final var method = String.valueOf(failedRequest.get("method"));
                        sendResponse(writer, id, null, Map.of(
                                "code", "REQUEST_FAILED",
                                "message", method + " failed: " + summarizeException(e)
                        ));
                    } catch (Exception responseError) {
                        error("Failed to send error response: " + summarizeException(responseError));
                    }
                }
            }

            if (connection != null && !connection.isClosed()) {
                connection.close();
            }
            if (mongoClient != null) {
                mongoClient.close();
            }
        } catch (Exception e) {
            error("Fatal error: " + summarizeException(e));
            System.exit(1);
        }
    }

    private static void connectBackend() throws Exception {
        if ("jdbc".equals(backend)) {
            Class.forName(driverClass);
            DriverManager.setLoginTimeout(3);
            log("Connecting JDBC: url=" + databaseUrl + " user=" + user + " driver=" + driverClass);
            connection = DriverManager.getConnection(databaseUrl, user, password);
            applyStatementTimeout();
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
        
        // Handle Clob
        if (val instanceof java.sql.Clob) {
            return ((java.sql.Clob) val).getSubString(1, (int) ((java.sql.Clob) val).length());
        }
        
        String className = val.getClass().getName();
        
        // Handle PGobject (jsonb, hstore, etc.) via reflection
        if (className.startsWith("org.postgresql.util.PGobject")) {
            try {
                java.lang.reflect.Method getValue = val.getClass().getMethod("getValue");
                String pgValue = (String) getValue.invoke(val);
                
                // Get type via reflection
                java.lang.reflect.Method getType = val.getClass().getMethod("getType");
                String pgType = (String) getType.invoke(val);
                
                // Parse JSON types
                if ("jsonb".equals(pgType) || "json".equals(pgType)) {
                    return MAPPER.readValue(pgValue, Object.class);
                }
                return pgValue;
            } catch (Exception e) {
                log("[CONVERT] Failed to convert PGobject: " + e.getMessage());
                return val.toString();
            }
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
        
        // Handle other types with getValue method (legacy support)
        try {
            java.lang.reflect.Method getValue = val.getClass().getMethod("getValue");
            Object extracted = getValue.invoke(val);
            return convertPgObject(extracted);
        } catch (NoSuchMethodException | SecurityException e) {
            // Not a PGobject or similar — keep original value
            return val;
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
            if (mongoClient != null) {
                sendResponse(writer, id, Map.of("status", "already_connected"), null);
                return;
            }
            String url = databaseUrl.replace("__SAFESELECT_PASSWORD__", URLEncoder.encode(password == null ? "" : password, java.nio.charset.StandardCharsets.UTF_8));
            mongoClient = MongoClients.create(url);
            sendResponse(writer, id, Map.of("status", "connected"), null);
            return;
        }
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
        sendResponse(writer, id, result, null);
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

        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        MongoCollection<Document> collection = mongoClient.getDatabase(databaseName).getCollection(collectionName);
        FindIterable<Document> find = collection.find(filter);

        Object projection = params.get("projection");
        if (projection != null) {
            find.projection(toDocument(projection));
        }
        Object sort = params.get("sort");
        if (sort != null) {
            find.sort(toDocument(sort));
        }
        long requestedLimit = ((Number) params.getOrDefault("limit", Math.min(maxRows, 100))).longValue();
        int effectiveLimit = (int) Math.min(requestedLimit, maxRows);
        find.limit(effectiveLimit);

        List<Object> documents = new ArrayList<>();
        long byteCount = 0;
        try (MongoCursor<Document> cursor = find.iterator()) {
            while (cursor.hasNext()) {
                Document document = cursor.next();
                Object converted = MAPPER.readValue(document.toJson(), Object.class);
                long documentBytes = MAPPER.writeValueAsBytes(converted).length;
                if (documents.size() >= maxRows) {
                    sendResponse(writer, id, null, Map.of(
                            "code", "RESULT_LIMIT_EXCEEDED",
                            "message", "Document limit exceeded: " + maxRows,
                            "limit_type", "max_rows",
                            "limit_value", maxRows
                    ));
                    return;
                }
                if (byteCount + documentBytes > maxResultBytes) {
                    sendResponse(writer, id, null, Map.of(
                            "code", "RESULT_LIMIT_EXCEEDED",
                            "message", "Result size limit exceeded: " + maxResultBytes + " bytes",
                            "limit_type", "max_result_bytes",
                            "limit_value", maxResultBytes
                    ));
                    return;
                }
                byteCount += documentBytes;
                documents.add(converted);
            }
        }

        long elapsedMs = System.currentTimeMillis() - startTime;
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("documents", documents);
        result.put("document_count", documents.size());
        result.put("byte_count", byteCount);
        result.put("elapsed_ms", elapsedMs);
        result.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, result, null);
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

        List<Document> pipeline = new ArrayList<>();
        for (Object stage : rawPipeline) {
            Document document = toDocument(stage);
            for (String key : document.keySet()) {
                if ("$out".equals(key) || "$merge".equals(key) || "$currentOp".equals(key)) {
                    sendResponse(writer, id, null, Map.of("code", "NOT_READ_ONLY", "message", "Aggregation stage is not read-only: " + key));
                    return;
                }
            }
            pipeline.add(document);
        }
        long requestedLimit = numberParam(params, "limit", Math.min(maxRows, 100));
        int effectiveLimit = (int) Math.min(requestedLimit, maxRows);
        pipeline.add(new Document("$limit", effectiveLimit));

        MongoCollection<Document> collection = mongoClient.getDatabase(databaseName).getCollection(collectionName);
        AggregateIterable<Document> aggregate = collection.aggregate(pipeline).allowDiskUse(false);
        sendDocumentIterable(writer, id, aggregate.iterator(), startTime, "documents", "document_count");
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
        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        long limit = numberParam(params, "limit", Math.min(maxRows, 100));
        MongoCollection<Document> collection = mongoClient.getDatabase(databaseName).getCollection(collectionName);
        List<Object> values = new ArrayList<>();
        long byteCount = 0;
        try (MongoCursor<BsonValue> cursor = collection.distinct(field, filter, BsonValue.class).iterator()) {
            while (cursor.hasNext() && values.size() < limit && values.size() < maxRows) {
                Object value = convertBsonValue(cursor.next());
                long valueBytes = MAPPER.writeValueAsBytes(value).length;
                if (byteCount + valueBytes > maxResultBytes) {
                    sendLimitExceeded(writer, id, "max_result_bytes", maxResultBytes);
                    return;
                }
                byteCount += valueBytes;
                values.add(value);
            }
        }
        long elapsedMs = System.currentTimeMillis() - startTime;
        Map<String, Object> result = new LinkedHashMap<>();
        result.put("values", values);
        result.put("value_count", values.size());
        result.put("byte_count", byteCount);
        result.put("elapsed_ms", elapsedMs);
        result.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, result, null);
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
        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        long count = mongoClient.getDatabase(databaseName).getCollection(collectionName).countDocuments(filter);
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
        Document find = new Document("find", collectionName).append("filter", toDocument(params.getOrDefault("filter", Map.of())));
        if (params.get("projection") != null) {
            find.append("projection", toDocument(params.get("projection")));
        }
        if (params.get("sort") != null) {
            find.append("sort", toDocument(params.get("sort")));
        }
        if (params.get("limit") != null) {
            find.append("limit", numberParam(params, "limit", Math.min(maxRows, 100)));
        }
        Document explain = new Document("explain", find);
        Document result = mongoClient.getDatabase(databaseName).runCommand(explain, ReadPreference.secondaryPreferred());
        Map<String, Object> response = new LinkedHashMap<>();
        response.put("explain", convertBsonValue(result));
        long elapsedMs = System.currentTimeMillis() - startTime;
        response.put("elapsed_ms", elapsedMs);
        response.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, response, null);
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
        long sampleSize = numberParam(params, "sample_size", Math.min(maxRows, 1000));
        long exampleLimit = numberParam(params, "examples", 5);
        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        MongoCollection<Document> collection = mongoClient.getDatabase(databaseName).getCollection(collectionName);
        FieldStats stats = new FieldStats(exampleLimit);
        long scanned = 0;
        try (MongoCursor<Document> cursor = collection.find(filter).limit((int) Math.min(sampleSize, maxRows)).iterator()) {
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
        long sampleSize = numberParam(params, "sample_size", Math.min(maxRows, 1000));
        long exampleLimit = numberParam(params, "examples", 3);
        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        MongoCollection<Document> collection = mongoClient.getDatabase(databaseName).getCollection(collectionName);
        Map<String, FieldStats> fields = new TreeMap<>();
        long scanned = 0;
        try (MongoCursor<Document> cursor = collection.find(filter).limit((int) Math.min(sampleSize, maxRows)).iterator()) {
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
        Document filter = toDocument(params.getOrDefault("filter", Map.of()));
        long limit = numberParam(params, "limit", Math.min(maxRows, 20));
        FindIterable<Document> find = mongoClient.getDatabase(databaseName).getCollection(collectionName)
                .find(filter)
                .limit((int) Math.min(limit, maxRows));
        if (params.get("projection") != null) {
            find.projection(toDocument(params.get("projection")));
        }
        List<String> redactFields = new ArrayList<>();
        if (params.get("redact_fields") instanceof List<?> fields) {
            for (Object field : fields) {
                if (field instanceof String value) {
                    redactFields.add(value);
                }
            }
        }
        List<Object> documents = new ArrayList<>();
        long byteCount = 0;
        try (MongoCursor<Document> cursor = find.iterator()) {
            while (cursor.hasNext()) {
                Object converted = convertBsonValue(cursor.next());
                redactValue(converted, "", redactFields);
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
        result.put("documents", documents);
        result.put("document_count", documents.size());
        result.put("byte_count", byteCount);
        result.put("redacted_fields", redactFields);
        result.put("elapsed_ms", elapsedMs);
        result.put("elapsed", formatElapsed(elapsedMs));
        sendResponse(writer, id, result, null);
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
        if (value.isString()) {
            return value.asString().getValue();
        }
        if (value.isBoolean()) {
            return value.asBoolean().getValue();
        }
        if (value.isInt32()) {
            return value.asInt32().getValue();
        }
        if (value.isInt64()) {
            return value.asInt64().getValue();
        }
        if (value.isDouble()) {
            return value.asDouble().getValue();
        }
        if (value.isDecimal128()) {
            return value.asDecimal128().getValue().bigDecimalValue();
        }
        if (value.isObjectId()) {
            return value.asObjectId().getValue().toHexString();
        }
        if (value.isDateTime()) {
            return value.asDateTime().getValue();
        }
        if (value.isTimestamp()) {
            return value.asTimestamp().getValue();
        }
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
        if (value instanceof Map<?, ?> map) {
            if (!map.containsKey(parts[index])) {
                return MissingValue.INSTANCE;
            }
            return resolvePath(map.get(parts[index]), parts, index + 1);
        }
        if (value instanceof Document document) {
            if (!document.containsKey(parts[index])) {
                return MissingValue.INSTANCE;
            }
            return resolvePath(document.get(parts[index]), parts, index + 1);
        }
        if (value instanceof List<?> list) {
            List<Object> values = new ArrayList<>();
            for (Object item : list) {
                Object nested = resolvePath(item, parts, index);
                if (nested != MissingValue.INSTANCE) {
                    values.add(nested);
                }
            }
            if (values.isEmpty()) {
                return MissingValue.INSTANCE;
            }
            return values;
        }
        return MissingValue.INSTANCE;
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

    @SuppressWarnings("unchecked")
    private static void handleExecute(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        long startTime = System.currentTimeMillis();
        log("[EXECUTE] Starting query execution, id=" + id);
        
        if (connection == null || connection.isClosed()) {
            error("Not connected, returning error");
            sendResponse(writer, id, null,
                    Map.of("code", "NOT_CONNECTED", "message", "Database not connected. Use 'connect' first."));
            return;
        }

        Map<String, Object> params = (Map<String, Object>) request.get("params");
        if (params == null) {
            error("Missing params");
            sendResponse(writer, id, null, Map.of("code", "MISSING_PARAMS", "message", "No params"));
            return;
        }

        String sql = (String) params.get("sql");
        if (sql == null || sql.isBlank()) {
            error("Missing SQL");
            sendResponse(writer, id, null, Map.of("code", "MISSING_SQL", "message", "No SQL provided"));
            return;
        }

        log("[EXECUTE] SQL: " + sql.substring(0, Math.min(100, sql.length())) + "...");

        try (Statement stmt = connection.createStatement()) {
            if (statementTimeoutMs > 0) {
                int timeoutSeconds = (int) Math.ceil(statementTimeoutMs / 1000.0);
                stmt.setQueryTimeout(timeoutSeconds);
            }
            log("[EXECUTE] Executing statement...");
            boolean isResultSet = stmt.execute(sql);
            log("[EXECUTE] Statement executed in " + (System.currentTimeMillis() - startTime) + "ms, isResultSet=" + isResultSet);

            if (isResultSet) {
                try (ResultSet rs = stmt.getResultSet()) {
                    ResultSetMetaData meta = rs.getMetaData();
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
                            sendResponse(writer, id, null, Map.of(
                                    "code", "RESULT_LIMIT_EXCEEDED",
                                    "message", "Row limit exceeded: " + maxRows,
                                    "limit_type", "max_rows",
                                    "limit_value", maxRows
                            ));
                            return;
                        }
                        List<Object> row = new ArrayList<>();
                        long rowBytes = 0;
                        for (int i = 1; i <= columnCount; i++) {
                            Object val = rs.getObject(i);
                            val = convertPgObject(val);
                            row.add(val);
                            if (val != null) {
                                rowBytes += val.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8).length;
                            }
                        }
                        if (byteCount + rowBytes > maxResultBytes) {
                            sendResponse(writer, id, null, Map.of(
                                    "code", "RESULT_LIMIT_EXCEEDED",
                                    "message", "Result size limit exceeded: " + maxResultBytes + " bytes",
                                    "limit_type", "max_result_bytes",
                                    "limit_value", maxResultBytes
                            ));
                            return;
                        }
                        byteCount += rowBytes;
                        rows.add(row);
                        rowCount++;
                    }
                    long elapsedMs = System.currentTimeMillis() - startTime;
                    log("[EXECUTE] Read " + rowCount + " rows, " + byteCount + " bytes in " + elapsedMs + "ms");

                    Map<String, Object> result = new LinkedHashMap<>();
                    result.put("columns", columns);
                    result.put("rows", rows);
                    result.put("row_count", rowCount);
                    result.put("byte_count", byteCount);
                    result.put("elapsed_ms", elapsedMs);
                    result.put("elapsed", formatElapsed(elapsedMs));

                    log("[EXECUTE] Sending response...");
                    sendResponse(writer, id, result, null);
                    log("[EXECUTE] Completed in " + elapsedMs + "ms");
                }
            } else {
                long elapsedMs = System.currentTimeMillis() - startTime;
                Map<String, Object> result = new LinkedHashMap<>();
                result.put("elapsed_ms", elapsedMs);
                result.put("elapsed", formatElapsed(elapsedMs));
                log("[EXECUTE] Non-result statement completed in " + elapsedMs + "ms");
                sendResponse(writer, id, result, null);
            }
        } catch (SQLException e) {
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
