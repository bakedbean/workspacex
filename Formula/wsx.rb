class Wsx < Formula
  desc "Terminal UI for managing coding agent sessions in git worktrees"
  homepage "https://github.com/bakedbean/workspacex"
  version "0.1.7"
  license "MIT"

  # The checksums below are placeholders until the first tagged release.
  # The `homebrew` job in .github/workflows/release.yml rewrites the version,
  # the urls, and every sha256 through scripts/update-homebrew-formula.sh,
  # then opens a pull request with the result.
  on_macos do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.7/wsx-0.1.7-aarch64-apple-darwin.tar.gz"
      sha256 "c4f793a724c0ee0e94fcf612e8d7348cec6fc9cb0cd3f339a738664863c109f8"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.7/wsx-0.1.7-x86_64-apple-darwin.tar.gz"
      sha256 "a650caac1751185a0b6839106b25ec97382f9a3547a91254f0f289e8495ffaea"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.7/wsx-0.1.7-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "350d81bd83f9c6f44723895247397fa896b2babe739734444a129dfdca458bf7"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.7/wsx-0.1.7-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "1f06321ae68026c8468d970cf620521ca85b1e8c014823a1d89fcabcba83566a"
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
