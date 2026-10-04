import SwiftUI
import LucidCore

struct SettingsView: View {
    @StateObject private var model = SettingsModel()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header
                serviceCard
                modelCard
                actionCard
                inputMethodCard
                usageCard
            }
            .padding(28)
            .frame(maxWidth: 760, alignment: .leading)
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .frame(minWidth: 680, idealWidth: 720, minHeight: 650, idealHeight: 720)
    }

    private var header: some View {
        HStack(spacing: 14) {
            Image("LucidIcon")
                .resizable()
                .interpolation(.high)
                .aspectRatio(contentMode: .fit)
                .frame(width: 58, height: 58)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                .shadow(color: .black.opacity(0.18), radius: 8, y: 3)

            VStack(alignment: .leading, spacing: 4) {
                Text("Lucid")
                    .font(.title.weight(.semibold))
                Text("让中文思路自然变成英文表达")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            Spacer()
        }
    }

    private var serviceCard: some View {
        SettingsCard(title: "AI 服务", systemImage: "network") {
            VStack(alignment: .leading, spacing: 14) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("接口协议")
                        .font(.subheadline.weight(.medium))
                    Picker("接口协议", selection: $model.apiProtocol) {
                        Text("OpenAI 兼容").tag(AIProtocol.openAICompatible)
                        Text("Anthropic 兼容").tag(AIProtocol.anthropicCompatible)
                    }
                    .pickerStyle(.segmented)
                    .onChange(of: model.apiProtocol) { _ in
                        model.clearAvailableModels()
                    }
                }

                VStack(alignment: .leading, spacing: 6) {
                    Text("AI 服务地址")
                        .font(.subheadline.weight(.medium))
                    TextField("例如 https://api.example.com", text: $model.baseURL)
                        .textFieldStyle(.roundedBorder)
                        .onChange(of: model.baseURL) { _ in
                            model.clearAvailableModels()
                        }
                    Text("填写服务商提供的 API 地址。通常填写到域名或 /v1，不要填写具体的 /models 路径。")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text("API Key")
                            .font(.subheadline.weight(.medium))
                        if model.keyConfigured {
                            StatusBadge(text: "已配置", color: .green)
                        }
                    }
                    SecureField(model.keyConfigured ? "已保存 Key；输入新 Key 可替换" : "粘贴 API Key", text: $model.apiKey)
                        .textFieldStyle(.roundedBorder)
                    Text("Key 只保存在 Lucid 自己的设置里，不会写入钥匙串。")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    private var modelCard: some View {
        SettingsCard(title: "选择模型", systemImage: "cube") {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .firstTextBaseline) {
                    Text("模型由服务端提供")
                        .font(.subheadline.weight(.medium))
                    if !model.availableModels.isEmpty {
                        StatusBadge(text: "\(model.availableModels.count) 个可用", color: .blue)
                    }
                    Spacer()
                    Button {
                        model.fetchModels()
                    } label: {
                        Label(model.isFetchingModels ? "获取中…" : "获取模型列表", systemImage: "arrow.clockwise")
                    }
                    .buttonStyle(.bordered)
                    .disabled(model.isFetchingModels)
                    if model.isFetchingModels {
                        ProgressView()
                            .controlSize(.small)
                    }
                }

                if model.availableModels.isEmpty {
                    VStack(alignment: .leading, spacing: 5) {
                        Label("尚未加载模型列表", systemImage: "info.circle")
                            .font(.callout)
                        if !model.model.isEmpty {
                            Text("当前已保存：\(model.model)。获取列表后即可改为从下拉菜单选择。")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        } else {
                            Text("先填写 AI 服务地址和 API Key，再点击“获取模型列表”。")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 9))
                } else {
                    Picker("使用模型", selection: $model.model) {
                        ForEach(model.availableModels, id: \.self) { name in
                            Text(name).tag(name)
                        }
                    }
                    .pickerStyle(.menu)
                    .labelsHidden()
                    .frame(maxWidth: .infinity, alignment: .leading)

                    Text("已获取的模型会保留在当前设置中；更换服务地址或协议后，请重新获取列表。")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    private var actionCard: some View {
        SettingsCard(title: "保存并检查", systemImage: "checkmark.shield") {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 10) {
                    Button("保存设置", action: model.save)
                        .buttonStyle(.borderedProminent)
                        .keyboardShortcut(.defaultAction)
                        .disabled(model.model.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    Button("测试连接", action: model.testConnection)
                        .buttonStyle(.bordered)
                        .disabled(model.isTesting || model.model.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    if model.isTesting {
                        ProgressView()
                            .controlSize(.small)
                    }
                }

                if !model.statusMessage.isEmpty {
                    Label(model.statusMessage, systemImage: statusIcon)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
        }
    }

    private var inputMethodCard: some View {
        SettingsCard(title: "启用输入法", systemImage: "keyboard") {
            VStack(alignment: .leading, spacing: 12) {
                Text(model.inputMethodStatus)
                    .font(.callout)
                    .foregroundStyle(.secondary)

                HStack(spacing: 10) {
                    Button("启用 Lucid", action: model.enableInputMethod)
                        .buttonStyle(.borderedProminent)
                    Button("打开系统键盘设置", action: model.openKeyboardSettings)
                        .buttonStyle(.bordered)
                }

                Text("安装后如果在输入法列表中看不到 Lucid，请先重新登录 macOS，再到“系统设置 → 键盘 → 文本输入”中添加。添加后可从菜单栏输入法菜单切换。")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var usageCard: some View {
        SettingsCard(title: "使用说明", systemImage: "sparkles") {
            VStack(alignment: .leading, spacing: 8) {
                UsageRow(number: "1", text: "切换到 Lucid 输入法，在任意文本框中输入中文、拼音或混合内容。")
                UsageRow(number: "2", text: "输入完整句子后，输入句号、问号、感叹号，或停止输入约两秒；不需要按回车。")
                UsageRow(number: "3", text: "Lucid 会先显示英文建议，不会自动替换原文。点击“使用英文”才会替换，点击“保留原文”则继续使用原内容。")
            }
        }
    }

    private var statusIcon: String {
        let message = model.statusMessage
        if message.contains("成功") || message.contains("已保存") || message.contains("已获取") {
            return "checkmark.circle"
        }
        if message.contains("正在") || message.contains("获取中") {
            return "hourglass"
        }
        return "exclamationmark.circle"
    }
}

private struct SettingsCard<Content: View>: View {
    let title: String
    let systemImage: String
    @ViewBuilder let content: () -> Content

    var body: some View {
        GroupBox {
            content()
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.top, 3)
        } label: {
            Label(title, systemImage: systemImage)
                .font(.headline)
        }
    }
}

private struct StatusBadge: View {
    let text: String
    let color: Color

    var body: some View {
        Text(text)
            .font(.caption.weight(.medium))
            .foregroundStyle(color)
            .padding(.horizontal, 7)
            .padding(.vertical, 3)
            .background(color.opacity(0.12), in: Capsule())
    }
}

private struct UsageRow: View {
    let number: String
    let text: String

    var body: some View {
        HStack(alignment: .top, spacing: 9) {
            Text(number)
                .font(.caption.weight(.bold))
                .foregroundStyle(.white)
                .frame(width: 20, height: 20)
                .background(Color.accentColor, in: Circle())
            Text(text)
                .font(.callout)
                .foregroundStyle(.secondary)
        }
    }
}
