class Safeselect < Formula
  desc "MCP SQL Fail-Closed for AI Agents"
  homepage "https://github.com/antonillos/safeselect"
  license "MIT OR Apache-2.0"

  if Hardware::CPU.arm?
    url "https://github.com/antonillos/safeselect/releases/download/v0.1.0/safeselect-aarch64-apple-darwin.tar.gz"
    sha256 "PLACEHOLDER_ARM64"
  else
    url "https://github.com/antonillos/safeselect/releases/download/v0.1.0/safeselect-x86_64-apple-darwin.tar.gz"
    sha256 "PLACEHOLDER_X86_64"
  end

  depends_on "openjdk@17"

  def install
    bin.install "safeselect"
  end

  def caveats
    <<~EOS
      SafeSelect has been installed. To get started:

        safeselect --help

      A JDBC driver is required. Download the PostgreSQL driver:

        safeselect driver download --vendor postgresql

      Or register a custom driver:

        safeselect driver add --vendor <name> --path /path/to/jdbc.jar --class <driver-class>

      For MCP (Model Context Protocol) support, install the integration:

        safeselect agent install opencode --project <project> --environment <env> --name <name>
    EOS
  end

  test do
    assert_match "safeselect #{version}", shell_output("#{bin}/safeselect --version")
  end
end
