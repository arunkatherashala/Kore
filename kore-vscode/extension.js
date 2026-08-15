const vscode = require('vscode');
const { execSync } = require('child_process');
const path = require('path');

function activate(context) {
    // Preview command
    context.subscriptions.push(
        vscode.commands.registerCommand('kore.preview', () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) return;
            const filePath = editor.document.uri.fsPath;
            if (!filePath.endsWith('.hkore') && !filePath.endsWith('.kore')) {
                vscode.window.showErrorMessage('Not a .kore/.hkore file');
                return;
            }
            const panel = vscode.window.createWebviewPanel('korePreview', 'KORE Preview', vscode.ViewColumn.Beside, {});
            try {
                const pythonPath = 'python';
                const script = `
import sys, json
sys.path.insert(0, '${path.join(__dirname, '..', 'kore-python').replace(/\\/g, '/')}')
import kore_fileformat as kore
header = kore.read_hybrid_header('${filePath.replace(/\\/g, '/')}')
data = kore.read_hybrid('${filePath.replace(/\\/g, '/')}')
cols = [{'name': c.name, 'data': list(c.data)[:100]} for c in data.columns]
print(json.dumps({'header': header, 'rows': data.num_rows, 'cols': len(data.columns), 'columns': cols}))
`;
                const result = execSync(`${pythonPath} -c "${script}"`, { encoding: 'utf-8', maxBuffer: 10 * 1024 * 1024 });
                const info = JSON.parse(result.trim());
                let tableRows = '';
                const maxRows = Math.min(100, info.columns[0]?.data?.length || 0);
                for (let i = 0; i < maxRows; i++) {
                    tableRows += '<tr>' + info.columns.map(c => `<td>${c.data[i]}</td>`).join('') + '</tr>';
                }
                panel.webview.html = `<!DOCTYPE html><html><head><style>
                    body { font-family: monospace; padding: 20px; }
                    table { border-collapse: collapse; width: 100%; }
                    th, td { border: 1px solid #444; padding: 6px 10px; text-align: left; }
                    th { background: #1a73e8; color: white; }
                    tr:nth-child(even) { background: #f5f5f5; }
                    .header { white-space: pre; background: #1e1e1e; color: #d4d4d4; padding: 15px; border-radius: 8px; margin-bottom: 20px; }
                    h2 { color: #1a73e8; }
                </style></head><body>
                    <h2>KORE FileFormat Preview</h2>
                    <p>${info.rows.toLocaleString()} rows × ${info.cols} columns</p>
                    <div class="header">${info.header.replace(/\n/g, '<br>')}</div>
                    <table>
                        <tr>${info.columns.map(c => `<th>${c.name}</th>`).join('')}</tr>
                        ${tableRows}
                    </table>
                    <p><i>Showing first ${maxRows} of ${info.rows.toLocaleString()} rows</i></p>
                </body></html>`;
            } catch (e) {
                panel.webview.html = `<h2>Error</h2><pre>${e.message}</pre>`;
            }
        })
    );

    // Inspect command
    context.subscriptions.push(
        vscode.commands.registerCommand('kore.inspect', () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) return;
            const filePath = editor.document.uri.fsPath;
            vscode.window.showInformationMessage(`KORE: ${filePath} — use Preview for details`);
        })
    );
}

function deactivate() {}

module.exports = { activate, deactivate };
