package com.safeselect;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;

import java.net.ConnectException;
import java.net.SocketTimeoutException;
import java.sql.SQLException;
import java.sql.SQLTimeoutException;
import java.util.List;
import javax.net.ssl.SSLHandshakeException;
import org.junit.jupiter.api.Test;

class ConnectionFailureTest {
    private static final String PRIVATE_DETAIL = "synthetic-private-host-user-password";

    @Test
    void preservesAuthenticationSqlstateWithoutExceptionDetails() {
        final var failure = ConnectionFailure.from(new SQLException(PRIVATE_DETAIL, "28P01"));
        assertEquals("authentication", failure.category());
        assertEquals("28P01", failure.sqlState());
        assertEquals("28P01", failure.startupResponse().get("sql_state"));
        assertFalse(failure.startupResponse().toString().contains(PRIVATE_DETAIL));
        assertFalse(failure.logMessage().contains(PRIVATE_DETAIL));
    }

    @Test
    void distinguishesDriverTlsTimeoutAndNetworkFailures() {
        for (final var example : List.of(
                new Example(new ClassNotFoundException(PRIVATE_DETAIL), "driver"),
                new Example(new SQLException("No suitable driver: " + PRIVATE_DETAIL, "08001"), "driver"),
                new Example(new SQLTimeoutException(PRIVATE_DETAIL), "timeout"),
                new Example(new SocketTimeoutException(PRIVATE_DETAIL), "timeout"),
                new Example(new SQLException(PRIVATE_DETAIL, "HYT00"), "timeout"),
                new Example(new ConnectException(PRIVATE_DETAIL), "network"),
                new Example(new SQLException(PRIVATE_DETAIL, "08006"), "network"),
                new Example(new SSLHandshakeException(PRIVATE_DETAIL), "tls"),
                new Example(new IllegalStateException(PRIVATE_DETAIL), "database"))) {
            final var failure = ConnectionFailure.from(new RuntimeException(PRIVATE_DETAIL, example.failure()));
            assertEquals(example.category(), failure.category());
            assertFalse(failure.logMessage().contains(PRIVATE_DETAIL));
            assertFalse(failure.startupResponse().toString().contains(PRIVATE_DETAIL));
        }
    }

    @Test
    void recognizesTlsAndTimeoutInsideGenericSqlConnectionErrors() {
        for (final var example : List.of(
                new Example(new SSLHandshakeException(PRIVATE_DETAIL), "tls"),
                new Example(new SocketTimeoutException(PRIVATE_DETAIL), "timeout"))) {
            final var failure = ConnectionFailure.from(new SQLException(PRIVATE_DETAIL, "08001", example.failure()));
            assertEquals(example.category(), failure.category());
            assertEquals("08001", failure.sqlState());
        }
    }

    @Test
    void discardsMalformedSqlstatesAndRetainsSafeFallback() {
        for (final var state : List.of(PRIVATE_DETAIL, "28p01", "28P01\n", "")) {
            final var failure = ConnectionFailure.from(new SQLException(PRIVATE_DETAIL, state));
            assertEquals("", failure.sqlState());
            assertFalse(failure.startupResponse().containsKey("sql_state"));
            assertFalse(failure.logMessage().contains(PRIVATE_DETAIL));
        }
        assertEquals("database connection failed; details redacted",
                ConnectionFailure.from(new IllegalStateException(PRIVATE_DETAIL)).logMessage());
    }

    private record Example(Throwable failure, String category) {}

    @Test
    void preservesSqlExceptionPrecedenceAndHandlesMissingDetails() {
        for (final var example : List.of(
                new Example(new SQLException(), "database"),
                new Example(new java.sql.SQLFeatureNotSupportedException(), "driver"),
                new Example(new SQLTimeoutException(PRIVATE_DETAIL, "28P01"), "timeout"),
                new Example(new SQLTimeoutException("No suitable driver", "HYT00"), "driver"),
                new Example(new SQLException(PRIVATE_DETAIL, "HYT01"), "timeout"),
                new Example(new java.net.UnknownHostException(PRIVATE_DETAIL), "network"),
                new Example(new java.net.NoRouteToHostException(PRIVATE_DETAIL), "network"))) {
            assertEquals(example.category(), ConnectionFailure.from(example.failure()).category());
        }
    }

    @Test
    void recognizesPostgresqlSslRefusalWithoutSslExceptionCause() {
        final var failure = ConnectionFailure.from(new SQLException("The server does not support SSL.", "08004"));
        assertEquals("tls", failure.category());
        assertEquals("08004", failure.sqlState());
        assertEquals("tls", failure.startupResponse().get("category"));
        for (final var other : List.of(
                new SQLException(PRIVATE_DETAIL, "08004"),
                new SQLException("SSL " + PRIVATE_DETAIL, "08004"),
                new SQLException("The server does not support SSL.", "28P01"))) {
            final var classified = ConnectionFailure.from(other);
            assertFalse(classified.category().equals("tls"));
            assertFalse(classified.logMessage().contains(PRIVATE_DETAIL));
        }
    }
}
