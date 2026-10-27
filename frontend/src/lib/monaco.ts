// A lean Monaco build: the core API, the standard editor features and C syntax
// highlighting, without the bundled TypeScript/CSS/HTML/JSON language services.
import * as monaco from 'monaco-editor/editor/editor.api'
import 'monaco-editor/features/register.all'
import 'monaco-editor/languages/definitions/cpp/register'
import EditorWorker from 'monaco-editor/editor/editor.worker?worker'

self.MonacoEnvironment = { getWorker: () => new EditorWorker() }

export { monaco }
