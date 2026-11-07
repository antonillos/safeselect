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
      Requires Java 17+ for database commands.

      Get started: https://github.com/antonillos/safeselect#quick-start
      CLI help:    safeselect --help
    EOS
  end

  test do
    assert_match "safeselect #{version}", shell_output("#{bin}/safeselect --version")
  end
end
