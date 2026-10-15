package com.safeselect;

import java.net.ConnectException;
import java.net.NoRouteToHostException;
import java.net.SocketTimeoutException;
import java.net.UnknownHostException;
import java.sql.SQLException;
import java.sql.SQLTimeoutException;
import java.util.LinkedHashMap;
import java.util.Map;
import javax.net.ssl.SSLException;

record ConnectionFailure(String category, String sqlState) {
    static ConnectionFailure from(Throwable failure) {
        String state = "";
        String category = "database";
        for (Throwable current = failure; current != null; current = current.getCause()) {
            if (current instanceof SQLException sql && state.isEmpty()) {
                final var candidate = sql.getSQLState();
                if (candidate != null && candidate.matches("[A-Z0-9]{5}")) {
                    state = candidate;
                }
            }
            final var candidate = categoryOf(current);
            if (priority(candidate) > priority(category)) {
                category = candidate;
            }
        }
        return new ConnectionFailure(category, state);
    }

    private static String categoryOf(Throwable failure) {
        if (failure instanceof SSLException
                || (failure instanceof SQLException sql
                    && "08004".equals(sql.getSQLState())
                    && "The server does not support SSL.".equals(sql.getMessage()))) {
            return "tls";
        }
        if (failure instanceof ClassNotFoundException
                || failure instanceof java.sql.SQLFeatureNotSupportedException
                || (failure instanceof SQLException && failure.getMessage() != null
                    && failure.getMessage().startsWith("No suitable driver"))) {
            return "driver";
        }
        if (failure instanceof SQLTimeoutException || failure instanceof SocketTimeoutException
                || failure instanceof com.mongodb.MongoTimeoutException) {
            return "timeout";
        }
        if (failure instanceof SQLException sql) {
            final var state = sql.getSQLState();
            if (state != null && state.startsWith("28")) {
                return "authentication";
            }
            if ("HYT00".equals(state) || "HYT01".equals(state)) {
                return "timeout";
            }
            if (state != null && state.startsWith("08")) {
                return "network";
            }
        }
        if (failure instanceof com.mongodb.MongoSecurityException) {
            return "authentication";
        }
        if (failure instanceof ConnectException || failure instanceof UnknownHostException
                || failure instanceof NoRouteToHostException
                || failure instanceof com.mongodb.MongoSocketException) {
            return "network";
        }
        return "database";
    }

    private static int priority(String category) {
        return switch (category) {
            case "tls" -> 5;
            case "authentication" -> 4;
            case "driver" -> 3;
            case "timeout" -> 2;
            case "network" -> 1;
            default -> 0;
        };
    }

    Map<String, String> startupResponse() {
        final var response = new LinkedHashMap<String, String>();
        response.put("type", "connection_error");
        response.put("category", this.category);
        if (!this.sqlState.isEmpty()) {
            response.put("sql_state", this.sqlState);
        }
        return response;
    }

    String logMessage() {
        if (this.category.equals("database") && this.sqlState.isEmpty()) {
            return "database connection failed; details redacted";
        }
        return "database connection failed; category=" + this.category
                + (this.sqlState.isEmpty() ? "" : "; SQLSTATE=" + this.sqlState)
                + "; details redacted";
    }
}
