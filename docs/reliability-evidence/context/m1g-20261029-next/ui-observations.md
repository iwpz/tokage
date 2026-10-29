# External UI observations

Transcribed from cua_repl native accessibility output and screenshots, not an injected driver. Screenshots remain in this chat's tool results; no standalone PNG is archived.

1. New app welcome source link and project Git status named f5ad465. Ordinary settings: Suggest, cache-aware on, 15%, Debug Off, Demo Off, timeout 3x. Only Tokage and Aider authorized for the external clone.
2. After three completed review turns: Context usage 184,740 / 200,000 (92%). Expanded details: “Hard limit exceeded (188740 tokens > 180000). Eliminating CONTINUE and recommending COMPACT”. Forecast costs: continue 0.004854, compact 0.014116, fresh 0.014263. “Start the next turn in a new window with retained context.” “Apply next turn” button.
3. Clicked Apply next turn once: “Queued: retained context will accompany the next turn. Compaction has not run.” Follow-up typed through ordinary composer and Send clicked once.
4. The send observation timed out/reset. Reconnected without resending. Completed carrier showed Prepared retained context for this turn; expanded text starts “Keep the current task goal, contract paths and unresolved errors” followed by retained-data JSON. Complete feedback: “Retained context was sent with this completed turn; actual context reduction is unverified.” Context estimate 6,795 / 200,000.
5. Normal Usage & Quality Dashboard: “Counterfactual Savings Ledger $0.00 Estimated Savings … Context first-turn counterfactual (CONTEXT) $0.00”; Total Spent $0.12; Aider DeepSeek Flash 4 calls, 339.8k tokens, $0.12.

The actual carrier content and completed cost are independently checked against the native CLI history and SQLite snapshot. See carrier-history-verification.json and settlement-verification.json. Earlier review analysis was not preserved in the fresh input; this accepts the implemented M1 retained-goal route only.
