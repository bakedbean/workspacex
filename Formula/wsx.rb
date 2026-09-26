class Wsx < Formula
  desc "Terminal UI for managing coding agent sessions in git worktrees"
  homepage "https://github.com/bakedbean/workspacex"
  version "0.1.1"
  license "MIT"

  # The checksums below are placeholders until the first tagged release.
  # The `homebrew` job in .github/workflows/release.yml rewrites the version,
  # the urls, and every sha256 through scripts/update-homebrew-formula.sh,
  # then opens a pull request with the result.
  on_macos do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.1/wsx-0.1.1-aarch64-apple-darwin.tar.gz"
      sha256 "7089b81bb914700dddd13e372fbf9444d28b1befd32ef4a6327a97d11f3bc4f9"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.1/wsx-0.1.1-x86_64-apple-darwin.tar.gz"
      sha256 "1d9a80c124fbfa265ca653dd38110afdf038faa6f330b2c22ea58342c631079b"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.1/wsx-0.1.1-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "4e2a358eeb63df0cf7a34977b74b9cada340955296a32cc85f7a874b73d496e8"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.1/wsx-0.1.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "b4b06d021c6f239880309e4dd038d66b358640b2e25c8c9ff7022c06e76fef0c"
    end
  end

  def install
    bin.install "wsx"
  end

  # wsx shells out to `git` and drives git worktrees, so git is a hard
  # runtime requirement. It is not declared as a dependency because macOS
  # ships git and Homebrew itself needs it on Linux, so it is always
  # present. The same reasoning is why lazygit and jj do not declare it.
  def caveats
    <<~EOS
      wsx needs `git` on your PATH.

      wsx reads pull request state through the GitHub CLI. Install it to see
      PR numbers and review marks on the dashboard:
        brew install gh
    EOS
  end

  test do
    assert_match "wsx #{version}", shell_output("#{bin}/wsx --version")
  end
end
