class Wsx < Formula
  desc "Terminal UI for managing coding agent sessions in git worktrees"
  homepage "https://github.com/bakedbean/workspacex"
  version "0.1.3"
  license "MIT"

  # The checksums below are placeholders until the first tagged release.
  # The `homebrew` job in .github/workflows/release.yml rewrites the version,
  # the urls, and every sha256 through scripts/update-homebrew-formula.sh,
  # then opens a pull request with the result.
  on_macos do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.3/wsx-0.1.3-aarch64-apple-darwin.tar.gz"
      sha256 "d70b286cdff4292ee0611b5d6b9d50e572cf31284298c8b63622e06eec189876"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.3/wsx-0.1.3-x86_64-apple-darwin.tar.gz"
      sha256 "fcaaf33aab23aef39f5a7b353ea6c3a60e38a84159f769ffadd540ecd5822e64"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.3/wsx-0.1.3-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "d502284df28c5722fd50053c3ac84c5575db11c759c93178b43933d6d18eea8d"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.3/wsx-0.1.3-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "198c18424cea282046febc36c85d86ac0e6677745f5b7957f4be481d733784a8"
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
