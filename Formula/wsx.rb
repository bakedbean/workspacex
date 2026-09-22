class Wsx < Formula
  desc "Terminal UI for managing coding agent sessions in git worktrees"
  homepage "https://github.com/bakedbean/workspacex"
  version "0.1.0"
  license "MIT"

  # The checksums below are placeholders until the first tagged release.
  # The `homebrew` job in .github/workflows/release.yml rewrites the version,
  # the urls, and every sha256 through scripts/update-homebrew-formula.sh,
  # then opens a pull request with the result.
  on_macos do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.0/wsx-0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "af4724f88c4411dfb1005684e0a60d782de3945a2de78af90b9a1d881a87809f"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.0/wsx-0.1.0-x86_64-apple-darwin.tar.gz"
      sha256 "20c2e1b54826edbcac0102bd5ebeb95a3bd27fda1cb30d25acf9d8f89c296e38"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.0/wsx-0.1.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "837e5c067789817e246bf0c4aa5562aff508347414774b5704f36525dc80ae21"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.0/wsx-0.1.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "0bd35fd1c505fc2dc1821dcce73e16afd8f7602ee8cbaaa92ec9dedb8106296e"
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
