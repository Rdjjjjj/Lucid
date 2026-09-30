cask "lucid" do
  version "0.1.3"
  sha256 "0418a410fddf807df02b8eb371fec46f3828bc72438b431d036dff3f39049f34"

  url "https://github.com/zebraic07-lab/Lucid/releases/download/v#{version}/Lucid-#{version}.pkg"
  name "Lucid"
  desc "AI-powered English writing assistant for macOS"
  homepage "https://github.com/zebraic07-lab/Lucid"

  depends_on macos: ">= :ventura"

  pkg "Lucid-#{version}.pkg"

  uninstall pkgutil: "io.github.rdj.lucid.installer"

  caveats <<~EOS
    安装后请注销并重新登录，再到「系统设置 → 键盘 → 文本输入 → 编辑」里启用 Lucid。
  EOS
end
