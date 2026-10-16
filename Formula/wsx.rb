class Wsx < Formula
  desc "Terminal UI for managing coding agent sessions in git worktrees"
  homepage "https://github.com/bakedbean/workspacex"
  version "0.1.6"
  license "MIT"

  # The checksums below are placeholders until the first tagged release.
  # The `homebrew` job in .github/workflows/release.yml rewrites the version,
  # the urls, and every sha256 through scripts/update-homebrew-formula.sh,
  # then opens a pull request with the result.
  on_macos do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.6/wsx-0.1.6-aarch64-apple-darwin.tar.gz"
      sha256 "fc8780dd93325850bcb200dffc8d1b4d42b3b74986714d12040237e36b13e098"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.6/wsx-0.1.6-x86_64-apple-darwin.tar.gz"
      sha256 "0dcbf63d93dd197d7a6480a26b1b2318e5460c1f5d95a183cb268b5bdb0df37d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.6/wsx-0.1.6-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "d114e5f33323b07ef44c91ce3e13dc6b6e62068e172c9abcf990c1178dab7c92"
    end
    on_intel do
      url "https://github.com/bakedbean/workspacex/releases/download/v0.1.6/wsx-0.1.6-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "2a9ceec788041f938c9614e3391a948011353d98081675df7e94d8908558c39f"
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
