cask "english-input" do
  version "0.1.3"
  sha256 "a4fbf206bef6c32d6d0631d97dc5a72e37bb39decf65b56e243b5fa548828014"

  url "https://github.com/rdj/EnglishInput/releases/download/v#{version}/EnglishInput-#{version}.pkg"
  name "English Input"
  desc "AI-powered English input method for macOS"
  homepage "https://github.com/rdj/EnglishInput"

  depends_on macos: ">= :ventura"

  pkg "EnglishInput-#{version}.pkg"

  uninstall pkgutil: "io.github.rdj.englishinput.installer"

  caveats <<~EOS
    安装后请注销并重新登录，再到「系统设置 → 键盘 → 文本输入 → 编辑」里启用 English Input。
  EOS
end
