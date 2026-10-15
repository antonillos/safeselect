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
        if (failure instanceof SSLException) {
            return "tls";
        }
        if (failure instanceof ClassNotFoundException) {
            return "driver";
        }
        if (failure instanceof SQLException sql) {
            return sqlCategory(sql);
        }
        if (isSocketTimeout(failure)) {
            return "timeout";
        }
        if (failure instanceof com.mongodb.MongoSecurityException) {
            return "authentication";
        }
        return isNetworkFailure(failure) ? "network" : "database";
    }

    private static String sqlCategory(SQLException failure) {
        final var state = java.util.Objects.toString(failure.getSQLState(), "");
        if (isSslRefusal(failure)) {
            return "tls";
        }
        if (isDriverFailure(failure)) {
            return "driver";
        }
        if (failure instanceof SQLTimeoutException) {
            return "timeout";
        }
        if (state.startsWith("28")) {
            return "authentication";
        }
        if (java.util.Set.of("HYT00", "HYT01").contains(state)) {
            return "timeout";
        }
        return state.startsWith("08") ? "network" : "database";
    }

    private static boolean isSslRefusal(SQLException failure) {
        return "08004".equals(failure.getSQLState())
                && "The server does not support SSL.".equals(failure.getMessage());
    }

    private static boolean isDriverFailure(SQLException failure) {
        return failure instanceof java.sql.SQLFeatureNotSupportedException
                || java.util.Objects.toString(failure.getMessage(), "").startsWith("No suitable driver");
    }

    private static boolean isNetworkFailure(Throwable failure) {
        return failure instanceof ConnectException || failure instanceof UnknownHostException
                || failure instanceof NoRouteToHostException
                || failure instanceof com.mongodb.MongoSocketException;
    }

    private static boolean isSocketTimeout(Throwable failure) {
        return failure instanceof SocketTimeoutException
                || failure instanceof com.mongodb.MongoTimeoutException;
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
