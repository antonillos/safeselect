package com.safeselect;

import com.fasterxml.jackson.databind.ObjectMapper;

import java.io.*;
import java.sql.*;
import java.time.Instant;
import java.util.*;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;

public class Main {
    private static final ObjectMapper MAPPER = new ObjectMapper();
    private static final AtomicBoolean RUNNING = new AtomicBoolean(true);
    private static Connection connection;
    private static String driverClass;
    private static String jdbcUrl;
    private static String user;
    private static String password;
    private static long idleTimeoutMs = 0;
    private static long statementTimeoutMs = 0;
    private static boolean verboseMode = false;
    private static final AtomicLong lastActivityMs = new AtomicLong(System.currentTimeMillis());
    private static PrintWriter logWriter;

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

    public static void main(String[] args) throws Exception {
        // Initialize file logger
        String logDir = System.getProperty("user.home") + "/.local/state/safeselect/logs";
        new File(logDir).mkdirs();
        String logFile = logDir + "/sidecar-" + System.currentTimeMillis() + ".log";
        logWriter = new PrintWriter(new FileWriter(logFile, true));
        log("Starting sidecar, log file: " + logFile);
        driverClass = null;
        jdbcUrl = null;
        user = null;
        boolean passwordStdin = false;

        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--driver" -> driverClass = args[++i];
                case "--url" -> jdbcUrl = args[++i];
                case "--user" -> user = args[++i];
                case "--password-stdin" -> passwordStdin = true;
                case "--idle-timeout-seconds" -> idleTimeoutMs = Long.parseLong(args[++i]) * 1000;
                case "--statement-timeout-ms" -> statementTimeoutMs = Long.parseLong(args[++i]);
                case "--verbose" -> verboseMode = true;
            }
        }

        if (driverClass == null || jdbcUrl == null || user == null || !passwordStdin) {
            error("Usage: --driver <class> --url <jdbc> --user <name> --password-stdin [--idle-timeout-seconds <sec>] [--statement-timeout-ms <ms>]");
            System.exit(1);
        }

        BufferedReader reader = new BufferedReader(new InputStreamReader(System.in));
        PrintWriter writer = new PrintWriter(new OutputStreamWriter(System.out));

        password = reader.readLine();
        if (password == null || password.isBlank()) {
            error("Password required on stdin");
            System.exit(1);
        }

        if (idleTimeoutMs > 0) {
            startIdleTimer(writer);
        }

        writer.println("ready");
        writer.flush();

        try {
            Class.forName(driverClass);
            log("Connecting: url=" + jdbcUrl + " user=" + user + " driver=" + driverClass);
            connection = DriverManager.getConnection(jdbcUrl, user, password);
            applyStatementTimeout();

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
                    error("Error processing request: " + e.getMessage());
                    e.printStackTrace(System.err);
                }
            }

            if (connection != null && !connection.isClosed()) {
                connection.close();
            }
        } catch (Exception e) {
            error("Fatal error: " + e.getMessage());
            e.printStackTrace(System.err);
            System.exit(1);
        }
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
        if (connection == null || connection.isClosed()) {
            sendResponse(writer, id, Map.of("status", "already_disconnected"), null);
            return;
        }
        connection.close();
        connection = null;
        sendResponse(writer, id, Map.of("status", "disconnected"), null);
    }

    private static void handleConnect(PrintWriter writer, Object id) throws Exception {
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
        connection = DriverManager.getConnection(jdbcUrl, user, password);
        applyStatementTimeout();
        sendResponse(writer, id, Map.of("status", "connected"), null);
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
                        List<Object> row = new ArrayList<>();
                        for (int i = 1; i <= columnCount; i++) {
                            Object val = rs.getObject(i);
                            val = convertPgObject(val);
                            row.add(val);
                            if (val != null) {
                                byteCount += val.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8).length;
                            }
                        }
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

                    log("[EXECUTE] Sending response...");
                    sendResponse(writer, id, result, null);
                    log("[EXECUTE] Completed in " + elapsedMs + "ms");
                }
            } else {
                int updateCount = stmt.getUpdateCount();
                long elapsedMs = System.currentTimeMillis() - startTime;
                Map<String, Object> result = new LinkedHashMap<>();
                result.put("affected_rows", updateCount);
                result.put("elapsed_ms", elapsedMs);
                log("[EXECUTE] Update query, affected_rows=" + updateCount + " in " + elapsedMs + "ms");
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
}
