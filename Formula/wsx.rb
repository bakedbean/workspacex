class Wsx < Formula
  desc "Terminal UI for managing coding agent sessions in git worktrees"
  homepage "https://github.com/bakedbean/workspacex"
  version "0.1.4"
  license "MIT"

  # The checksums below are placeholders until the first tagged release.
  # The `homebrew` job in .github/workflows/release.yml rewrites the version,
  # the urls, and every sha256 through scripts/update-homebrew-formula.sh,
  # then opens a pull request with the result.
  on_macos do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.4/wsx-0.1.4-aarch64-apple-darwin.tar.gz"
      sha256 "4b2901f98739bce1f73cbe369735ecc586064b8bee0b1049d4abaf53562e3640"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.4/wsx-0.1.4-x86_64-apple-darwin.tar.gz"
      sha256 "b02799ea8ffd6b1f60521bf51edf45de624766fbd7d16d2a6e92cd69a9975266"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.4/wsx-0.1.4-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "c74ed2986cfb60be3a3f5bfd48b96aa755f25fece9cd2c4fc861f0a45b8159f6"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.4/wsx-0.1.4-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "89d75a97c6c6173a73f462d254eeb3c081c98ae7487192cf046a7921c363b6f2"
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
