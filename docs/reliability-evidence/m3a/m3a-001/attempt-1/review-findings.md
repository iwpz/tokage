The original acceptance criteria are unchanged. This is a retry of m3a-001, not a new requirement.

Independent review of attempt 1 found that SessionHistoryView.test.tsx instantiated only a custom EventTarget and a scroll controller. It never rendered the React view or a real DOM element. It therefore did not satisfy the preregistered controllable DOM regression requirement. The tests also did not exercise loading failure, empty transcripts, stream end, attempt switching in the view, or the accessible return-to-latest control. Please meet the original acceptance with meaningful rendered DOM tests. A minimal DOM development dependency and its lockfile are already within scope and happy-dom has been prewarmed in the npm cache.

The new content wrapper also removed the existing vertical spacing between message cards: space-y-4 remained on the outer container rather than the messages' direct parent. Preserve the view's existing message spacing.

The first desktop gate failed before executing frontend checks because the runner omitted explicit confirmation for its offline npm ci setup command. The retry plan explicitly confirms that exact command, with scripts disabled; no safety policy is loosened. All original frontend gates remain required.

The native CLI sandbox intentionally denies writes to system temporary directories. Any test temporary files must remain in the writable workspace, and untracked temporary output must be cleaned before delivery. There is no need to access GUI automation or verify the running desktop from inside the executor: an independent real-stream desktop probe will be run against the resulting code before this requirement is counted as delivered. Do not claim that external verification already passed.
