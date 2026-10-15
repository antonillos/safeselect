package com.safeselect;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.sql.Connection;
import java.sql.Driver;
import java.sql.DriverManager;
import java.sql.DriverPropertyInfo;
import java.sql.SQLException;
import java.util.Map;
import java.util.Properties;
import java.util.concurrent.TimeUnit;
import java.util.logging.Logger;
import org.junit.jupiter.api.Test;
import tools.jackson.databind.ObjectMapper;

class MainStartupTest {
    @Test
    void startupEmitsAuthenticationCategoryAndSqlstateOverJsonLines() throws IOException, InterruptedException {
        this.assertStartupFailure(RejectedDriver.class.getName(), "authentication", "28P01");
    }

    @Test
    void startupDistinguishesMissingDriverFromAuthentication() throws IOException, InterruptedException {
        this.assertStartupFailure("synthetic.missing.Driver", "driver", null);
    }

    @Test
    void startupClassifiesDriverLinkageFailures() throws IOException, InterruptedException {
        this.assertStartupFailure(BrokenDriver.class.getName(), "driver", null);
    }

    private void assertStartupFailure(String driver, String category, String state) throws IOException, InterruptedException {
        final var java = Path.of(System.getProperty("java.home"), "bin", "java").toString();
        final var process = new ProcessBuilder(java, "-cp", System.getProperty("java.class.path"),
                Main.class.getName(), "--driver", driver,
                "--url", "jdbc:postgresql://synthetic-private-host/database",
                "--user", "synthetic-private-user", "--password-stdin").start();
        try {
            try (final var input = process.getOutputStream()) {
                input.write("synthetic-private-password\n".getBytes(StandardCharsets.UTF_8));
            }
            assertTrue(process.waitFor(10, TimeUnit.SECONDS), "sidecar must exit after startup rejection");
            assertEquals(1, process.exitValue());
            final var stdout = new String(process.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
            final var stderr = new String(process.getErrorStream().readAllBytes(), StandardCharsets.UTF_8);
            final var expected = state == null
                    ? Map.of("type", "connection_error", "category", category)
                    : Map.of("type", "connection_error", "category", category, "sql_state", state);
            assertEquals(expected, new ObjectMapper().readValue(stdout, Map.class));
            assertEquals(1, stdout.lines().count());
            assertFalse(stdout.contains("synthetic-private"));
            assertFalse(stderr.contains("synthetic-private"));
            assertFalse(stdout.contains("ready"));
        } finally {
            process.destroyForcibly();
            process.waitFor();
        }
    }

    public static final class RejectedDriver implements Driver {
        static {
            try {
                DriverManager.registerDriver(new RejectedDriver());
            } catch (SQLException e) {
                throw new ExceptionInInitializerError(e);
            }
        }

        @Override
        public Connection connect(String url, Properties properties) throws SQLException {
            throw new SQLException(url + properties, "28P01");
        }

        @Override
        public boolean acceptsURL(String url) { return url.startsWith("jdbc:postgresql:"); }

        @Override
        public DriverPropertyInfo[] getPropertyInfo(String url, Properties properties) { return new DriverPropertyInfo[0]; }

        @Override
        public int getMajorVersion() { return 1; }

        @Override
        public int getMinorVersion() { return 0; }

        @Override
        public boolean jdbcCompliant() { return false; }

        @Override
        public Logger getParentLogger() { return Logger.getGlobal(); }
    }

    public static final class BrokenDriver {
        static {
            failInitialization();
        }

        private static void failInitialization() {
            throw new NoClassDefFoundError("synthetic-private-driver-dependency");
        }
    }
}
