# Rendered by .github/workflows/release.yml; placeholders are filled per tag.
class Cctop < Formula
  desc "btop-style live dashboard for Claude Code internals, in a pane beside your session"
  homepage "https://github.com/@REPO@"
  version "@VERSION@"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/@REPO@/releases/download/v@VERSION@/cctop-@VERSION@-aarch64-apple-darwin.tar.gz"
      sha256 "@SHA_ARM_MAC@"
    else
      url "https://github.com/@REPO@/releases/download/v@VERSION@/cctop-@VERSION@-x86_64-apple-darwin.tar.gz"
      sha256 "@SHA_X86_MAC@"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/@REPO@/releases/download/v@VERSION@/cctop-@VERSION@-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA_ARM_LINUX@"
    else
      url "https://github.com/@REPO@/releases/download/v@VERSION@/cctop-@VERSION@-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA_X86_LINUX@"
    end
  end

  def install
    bin.install "cctop"
    pkgshare.install "plugin"
  end

  def caveats
    <<~EOS
      Claude Code plugin (adds /cctop and cctop-insights):
        claude plugin add #{pkgshare}/plugin
      Optional, for exact rate limits and tool timings:
        cctop install
    EOS
  end

  test do
    assert_match "cctop", shell_output("#{bin}/cctop --version")
    assert_match "cache_hit_ratio", shell_output("#{bin}/cctop query explain cache_hit_ratio")
  end
end
