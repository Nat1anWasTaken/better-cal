import { useMutation } from '@tanstack/react-query'
import { Info, Play, RotateCcw } from 'lucide-react'
import { useCallback, useMemo, useRef, useState } from 'react'
import { Navigate } from 'react-router'

import { api } from '@/api/client'
import type { PlaygroundResult, Span } from '@/api/types'
import { CardList } from '@/components/CardList'
import { CodeEditor, type CodeEditorHandle } from '@/components/CodeEditor'
import { EditorPreferencesPopover } from '@/components/EditorPreferencesPopover'
import { ErrorState, Loading } from '@/components/Page'
import { StatusPill } from '@/components/StatusPill'
import { Button } from '@/components/ui/button'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Textarea } from '@/components/ui/textarea'
import { useAuth } from '@/hooks/auth'
import { useSite } from '@/hooks/useSite'
import { useMediaQuery } from '@/hooks/useSubmission'
import { cardsToDiagnostics } from '@/lib/cards'
import { runStatusInfo } from '@/lib/format'
import { SAMPLES } from '@/lib/samples'

const STARTER = '#include <stdio.h>\n\nint main(void) {\n    printf("Hello, world!\\n");\n    return 0;\n}\n'

// Browser storage may be unavailable (private mode, blocked site data).
function load(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function store(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // Not saved; the page still works.
  }
}

function Output({ label, text, truncated, testid }: { label: string; text: string; truncated?: boolean; testid?: string }) {
  return (
    <div className="min-w-0">
      <div className="mb-1 text-xs font-medium text-muted-foreground">{label}</div>
      <pre data-testid={testid} className="max-h-72 overflow-auto rounded-md border bg-code p-2 font-mono text-xs leading-relaxed whitespace-pre-wrap">
        {text || <span className="text-muted-foreground italic">（沒有內容）</span>}
      </pre>
      {truncated && <p className="mt-1 text-sm text-warning">內容太長，只顯示前面的部分。</p>}
    </div>
  )
}

function Result({ r, onJump }: { r: PlaygroundResult; onJump: (span: Span) => void }) {
  // Hints are revealed locally: the server sends every hint this user may see.
  const [shown, setShown] = useState<Record<number, number>>({})
  const cards = useMemo(
    () => r.cards.map((c) => ({ ...c, hints: c.hints.slice(0, Math.max(r.auto_reveal_hints, shown[c.finding_id] ?? 0)) })),
    [r, shown],
  )
  const reveal = useCallback(
    async (id: number) => setShown((s) => ({ ...s, [id]: Math.max(r.auto_reveal_hints, s[id] ?? 0) + 1 })),
    [r.auto_reveal_hints],
  )
  const run = r.run
  const status = run ? runStatusInfo(run.status, run.exit_code) : { label: '無法編譯', tone: 'error' as const }
  return (
    <div className="space-y-4" data-testid="playground-result">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <StatusPill tone={status.tone} label={status.label} size="lg" />
        {run && (
          <span className="text-xs text-muted-foreground">
            {run.time_ms} ms · {Math.round(run.memory_kb / 1024)} MB
          </span>
        )}
      </div>
      {run && (
        <div className="space-y-3">
          <Output label="輸出（stdout）" text={run.stdout} truncated={run.stdout_truncated} testid="playground-stdout" />
          {run.stderr && <Output label="錯誤輸出（stderr）" text={run.stderr} truncated={run.stderr_truncated} />}
        </div>
      )}
      {cards.length > 0 ? (
        <CardList cards={cards} onJump={onJump} onRevealHint={reveal} rawExpanded={r.raw_expanded} />
      ) : (
        run?.status === 'ok' && <p className="text-sm text-muted-foreground">編譯器沒有任何警告。</p>
      )}
    </div>
  )
}

export function PlaygroundPage() {
  const { me } = useAuth()
  const site = useSite()
  const desktop = useMediaQuery('(min-width: 1024px)')
  const editor = useRef<CodeEditorHandle>(null)
  const keyBase = `ccal:playground:${me?.user.id ?? 'anon'}`
  // The document survives layout switches (the editor remounts between desktop and tabs).
  const code = useRef<string>(load(`${keyBase}:code`) ?? STARTER)
  const [stdin, setStdin] = useState(() => load(`${keyBase}:stdin`) ?? '')
  const [sample, setSample] = useState('')
  const [tab, setTab] = useState('editor')

  const run = useMutation({
    mutationFn: () => api.post<PlaygroundResult>('/playground/run', { source: editor.current?.getDoc() ?? code.current, stdin }),
    onSuccess: () => setTab('result'),
  })
  const { mutate, isPending } = run
  const start = useCallback(() => {
    if (!isPending) mutate()
  }, [isPending, mutate])

  const onChange = useCallback(
    (doc: string) => {
      code.current = doc
      store(`${keyBase}:code`, doc)
    },
    [keyBase],
  )
  const replace = (doc: string) => {
    editor.current?.reset(doc)
    onChange(doc)
    run.reset()
  }
  const onJump = useCallback(
    (span: Span) => {
      setTab('editor')
      // Wait for the tab to show the editor on narrow screens.
      window.setTimeout(() => editor.current?.jumpTo(span.line, span.column), desktop ? 0 : 50)
    },
    [desktop],
  )
  const diagnostics = useMemo(() => cardsToDiagnostics(run.data?.cards ?? []), [run.data])

  if (site.isLoading) return <Loading />
  if (site.data && !site.data.playground) return <Navigate to="/" replace />

  const toolbar = (
    <div className="flex flex-wrap items-center gap-2 border-b bg-card px-3 py-2">
      <Select
        value={sample}
        onValueChange={(v) => {
          setSample(v)
          const s = SAMPLES.find((x) => x.id === v)
          if (s) replace(s.code)
        }}
      >
        <SelectTrigger size="sm" className="w-44" aria-label="載入範例程式">
          <SelectValue placeholder="載入範例程式" />
        </SelectTrigger>
        <SelectContent>
          {SAMPLES.map((s) => (
            <SelectItem key={s.id} value={s.id}>
              {s.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Button
        variant="ghost"
        size="sm"
        className="ml-auto"
        aria-label="清空成 Hello world"
        onClick={() => {
          setSample('')
          replace(STARTER)
        }}
      >
        <RotateCcw aria-hidden />
        <span className="hidden xl:inline">重新開始</span>
      </Button>
      <EditorPreferencesPopover />
      <Button size="sm" onClick={start} disabled={isPending} title="Ctrl + Enter" data-testid="playground-run">
        <Play aria-hidden />
        {isPending ? '編譯執行中…' : '執行'}
      </Button>
    </div>
  )

  const editorPane = (
    <div className="flex h-full min-h-0 flex-col">
      {toolbar}
      <div className="min-h-0 flex-1 overflow-hidden">
        <CodeEditor
          key={String(desktop)}
          ref={editor}
          initialDoc={code.current}
          diagnostics={diagnostics}
          onChange={onChange}
          onRun={start}
          className="h-full"
          ariaLabel="練習場的程式碼"
        />
      </div>
    </div>
  )

  const stdinPane = (
    <div className="flex h-full min-h-0 flex-col gap-1.5 p-3">
      <label htmlFor="playground-stdin" className="text-xs font-medium text-muted-foreground">
        輸入（stdin）
      </label>
      <Textarea
        id="playground-stdin"
        variant="code"
        className="min-h-20 flex-1 resize-none"
        placeholder="在這裡輸入要給程式的資料，例如：3 5"
        value={stdin}
        onChange={(e) => {
          setStdin(e.target.value)
          store(`${keyBase}:stdin`, e.target.value)
        }}
      />
    </div>
  )

  const resultPane = (
    <div className="h-full overflow-y-auto p-4" aria-label="執行結果" aria-busy={isPending}>
      {run.error ? (
        <ErrorState error={run.error} />
      ) : run.data ? (
        <Result key={run.submittedAt} r={run.data} onJump={onJump} />
      ) : (
        <div className="space-y-2 text-muted-foreground">
          <p>寫好程式後按「執行」（Ctrl+Enter），輸出和說明會出現在這裡。</p>
          <p className="flex gap-1.5 text-sm">
            <Info aria-hidden className="mt-0.5 size-4 shrink-0" />
            <span>練習場用和評測相同的編譯器（GCC 12，C17）。程式不會被儲存成提交，也不會影響進度。</span>
          </p>
        </div>
      )}
    </div>
  )

  return (
    <div data-workspace className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b px-4 py-1.5 text-sm">
        <h1 className="font-medium">練習場</h1>
        <span className="text-muted-foreground">自由撰寫、編譯和執行 C 程式</span>
      </div>
      {desktop ? (
        <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
          <ResizablePanel defaultSize="58%" minSize="30%">
            {editorPane}
          </ResizablePanel>
          <ResizableHandle withHandle aria-label="調整程式欄寬度" />
          <ResizablePanel defaultSize="42%" minSize="25%">
            <ResizablePanelGroup orientation="vertical">
              <ResizablePanel defaultSize="25%" minSize="12%">
                {stdinPane}
              </ResizablePanel>
              <ResizableHandle withHandle aria-label="調整輸入區高度" />
              <ResizablePanel defaultSize="75%" minSize="25%">
                {resultPane}
              </ResizablePanel>
            </ResizablePanelGroup>
          </ResizablePanel>
        </ResizablePanelGroup>
      ) : (
        <Tabs value={tab} onValueChange={setTab} variant="flush" className="flex min-h-0 flex-1 flex-col">
          <TabsList className="mx-3 my-2 w-auto">
            <TabsTrigger value="editor">程式</TabsTrigger>
            <TabsTrigger value="stdin">輸入</TabsTrigger>
            <TabsTrigger value="result">結果</TabsTrigger>
          </TabsList>
          <TabsContent value="editor" forceMount className="min-h-editor flex-1 data-[state=inactive]:hidden">
            {editorPane}
          </TabsContent>
          <TabsContent value="stdin" className="min-h-0 flex-1">
            {stdinPane}
          </TabsContent>
          <TabsContent value="result" className="min-h-0 flex-1 overflow-y-auto">
            {resultPane}
          </TabsContent>
        </Tabs>
      )}
    </div>
  )
}
