class Safeselect < Formula
  desc "MCP SQL Fail-Closed for AI Agents"
  homepage "https://github.com/anomalyco/safeselect"
  license "MIT OR Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/anomalyco/safeselect/releases/download/vVERSION/safeselect-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_ARM64"
    else
      url "https://github.com/anomalyco/safeselect/releases/download/vVERSION/safeselect-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_X86_64"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/anomalyco/safeselect/releases/download/vVERSION/safeselect-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_LINUX_ARM64"
    else
      url "https://github.com/anomalyco/safeselect/releases/download/vVERSION/safeselect-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_LINUX_X86_64"
    end
  end

  depends_on "openjdk@17"

  def install
    bin.install "safeselect"
  end

  test do
    output = shell_output("#{bin}/safeselect --version")
    assert_match "safeselect #{version}", output
  end
end
