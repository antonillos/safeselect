package com.safeselect;

import com.fasterxml.jackson.databind.ObjectMapper;

import java.io.*;
import java.sql.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicBoolean;

public class Main {
    private static final ObjectMapper MAPPER = new ObjectMapper();
    private static final AtomicBoolean RUNNING = new AtomicBoolean(true);
    private static Connection connection;

    public static void main(String[] args) throws Exception {
        String driverClass = null;
        String jdbcUrl = null;
        String user = null;
        boolean passwordStdin = false;

        for (int i = 0; i < args.length; i++) {
            switch (args[i]) {
                case "--driver" -> driverClass = args[++i];
                case "--url" -> jdbcUrl = args[++i];
                case "--user" -> user = args[++i];
                case "--password-stdin" -> passwordStdin = true;
            }
        }

        if (driverClass == null || jdbcUrl == null || user == null || !passwordStdin) {
            System.err.println("Usage: --driver <class> --url <jdbc> --user <name> --password-stdin");
            System.exit(1);
        }

        BufferedReader reader = new BufferedReader(new InputStreamReader(System.in));
        PrintWriter writer = new PrintWriter(new OutputStreamWriter(System.out));

        String password = reader.readLine();
        if (password == null || password.isBlank()) {
            System.err.println("Password required on stdin");
            System.exit(1);
        }

        writer.println("ready");
        writer.flush();

        try {
            Class.forName(driverClass);
            connection = DriverManager.getConnection(jdbcUrl, user, password);

            while (RUNNING.get()) {
                String line = reader.readLine();
                if (line == null) break;

                try {
                    @SuppressWarnings("unchecked")
                    Map<String, Object> request = MAPPER.readValue(line, Map.class);
                    Object id = request.get("id");
                    String method = (String) request.get("method");

                    switch (method) {
                        case "ping" -> sendResponse(writer, id, "pong", null);
                        case "execute" -> handleExecute(writer, id, request);
                        case "shutdown" -> {
                            sendResponse(writer, id, "bye", null);
                            RUNNING.set(false);
                        }
                        default -> sendResponse(writer, id, null,
                                Map.of("code", "UNKNOWN_METHOD", "message", "Unknown method: " + method));
                    }
                } catch (Exception e) {
                    System.err.println("Error processing request: " + e.getMessage());
                    e.printStackTrace(System.err);
                }
            }

            if (connection != null && !connection.isClosed()) {
                connection.close();
            }
        } catch (Exception e) {
            System.err.println("Fatal error: " + e.getMessage());
            e.printStackTrace(System.err);
            System.exit(1);
        }
    }

    @SuppressWarnings("unchecked")
    private static void handleExecute(PrintWriter writer, Object id, Map<String, Object> request) throws Exception {
        Map<String, Object> params = (Map<String, Object>) request.get("params");
        if (params == null) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_PARAMS", "message", "No params"));
            return;
        }

        String sql = (String) params.get("sql");
        if (sql == null || sql.isBlank()) {
            sendResponse(writer, id, null, Map.of("code", "MISSING_SQL", "message", "No SQL provided"));
            return;
        }

        try (Statement stmt = connection.createStatement()) {
            boolean isResultSet = stmt.execute(sql);

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

                    while (rs.next()) {
                        List<Object> row = new ArrayList<>();
                        for (int i = 1; i <= columnCount; i++) {
                            Object val = rs.getObject(i);
                            row.add(val);
                            if (val != null) {
                                byteCount += val.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8).length;
                            }
                        }
                        rows.add(row);
                        rowCount++;
                    }

                    Map<String, Object> result = new LinkedHashMap<>();
                    result.put("columns", columns);
                    result.put("rows", rows);
                    result.put("row_count", rowCount);
                    result.put("byte_count", byteCount);

                    sendResponse(writer, id, result, null);
                }
            } else {
                int updateCount = stmt.getUpdateCount();
                Map<String, Object> result = new LinkedHashMap<>();
                result.put("affected_rows", updateCount);
                sendResponse(writer, id, result, null);
            }
        } catch (SQLException e) {
            Map<String, Object> error = new LinkedHashMap<>();
            error.put("code", "SQL_ERROR");
            error.put("message", e.getMessage());
            error.put("sql_state", e.getSQLState());
            error.put("error_code", e.getErrorCode());
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
