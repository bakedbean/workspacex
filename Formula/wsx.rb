class Wsx < Formula
  desc "Terminal UI for managing coding agent sessions in git worktrees"
  homepage "https://github.com/bakedbean/workspacex"
  version "0.1.5"
  license "MIT"

  # The checksums below are placeholders until the first tagged release.
  # The `homebrew` job in .github/workflows/release.yml rewrites the version,
  # the urls, and every sha256 through scripts/update-homebrew-formula.sh,
  # then opens a pull request with the result.
  on_macos do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.5/wsx-0.1.5-aarch64-apple-darwin.tar.gz"
      sha256 "a6afa3b81e4104992cc4b729a4eb222a03bb05ef1b3a522e3ec9804c6dff58a6"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.5/wsx-0.1.5-x86_64-apple-darwin.tar.gz"
      sha256 "1348ab86d3aa5ccf0e140cfacea86d590ab68e22baa6f7c8a099ec3faaad0548"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.5/wsx-0.1.5-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "f59be77b11dfe5c418d9af62143593c1e9d4c15dc68a77f516fc5ed2a90be833"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.5/wsx-0.1.5-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "fd367fb2fd61790b0b1c3b37c7ce16b10a717211709736e37077efdc632d8a4d"
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
