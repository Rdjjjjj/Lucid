import SwiftUI
import EnglishInputCore

struct SettingsView: View {
    @StateObject private var model = SettingsModel()

    var body: some View {
        Form {
            Section("AI 服务") {
                Picker("接口格式", selection: $model.apiProtocol) {
                    Text("OpenAI 兼容").tag(AIProtocol.openAICompatible)
                    Text("Anthropic 兼容").tag(AIProtocol.anthropicCompatible)
                }
                TextField("中转站地址", text: $model.baseURL)
                    .textContentType(.URL)
                    .textFieldStyle(.roundedBorder)
                TextField("模型名称", text: $model.model)
                    .textFieldStyle(.roundedBorder)
                SecureField(model.keyConfigured ? "已保存 Key；输入新 Key 可替换" : "API Key", text: $model.apiKey)
                    .textFieldStyle(.roundedBorder)
                Text("Key 仅保存在本机钥匙串。待处理句子会发送到所配置的中转站。")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section {
                HStack {
                    Button("保存设置", action: model.save)
                        .keyboardShortcut(.defaultAction)
                    Button("测试连接", action: model.testConnection)
                        .disabled(model.isTesting)
                    if model.isTesting { ProgressView().controlSize(.small) }
                }
                if !model.statusMessage.isEmpty {
                    Text(model.statusMessage)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }

            Section("试用") {
                Text("启用输入法后，可在聊天或其他文本框中试用英文输入和句末建议。")
                    .foregroundStyle(.secondary)
                Text("输入法启用引导将在系统级输入法原型完成后加入。")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .padding(20)
        .frame(minWidth: 520, minHeight: 360)
    }
}
