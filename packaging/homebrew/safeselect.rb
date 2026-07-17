class Safeselect < Formula
  desc "Fail-closed read-only database access for AI agents over MCP"
  homepage "https://github.com/antonillos/safeselect"
  license "MIT OR Apache-2.0"

  version "0.3.0"

  if Hardware::CPU.arm?
    url "https://github.com/antonillos/safeselect/releases/download/v#{version}/safeselect-v#{version}-aarch64-apple-darwin.tar.gz"
    sha256 "RELEASE_SHA256_ARM64"
  else
    url "https://github.com/antonillos/safeselect/releases/download/v#{version}/safeselect-v#{version}-x86_64-apple-darwin.tar.gz"
    sha256 "RELEASE_SHA256_X86_64"
  end

  def install
    bin.install "safeselect"
  end

  def caveats
    <<~EOS
      SafeSelect has been installed. To get started:

        safeselect --help

      SafeSelect requires Java 17 or newer at runtime. If needed, install it with:

        brew install openjdk@17

      PostgreSQL requires a JDBC driver. Download it with:

        safeselect driver download --vendor postgresql

      Or register a custom driver:

        safeselect driver add --vendor <name> --path /path/to/jdbc.jar --class <driver-class>

      For MCP (Model Context Protocol) support, install the integration:

        safeselect agent install opencode --environment <env> --name <name>

      (Run from your project repo — .safeselect/ is auto-detected.)
    EOS
  end

  test do
    assert_match "safeselect #{version}", shell_output("#{bin}/safeselect --version")
  end
end
