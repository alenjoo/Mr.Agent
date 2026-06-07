export const channels = [
    {
        label: "CLI Input",
        detail: "For focused repo work, shell-native prompts, and workspace-aware requests.",
        accent: "Workspace hint",
    },
    {
        label: "Telegram Capture",
        detail: "For mobile-first thoughts, interrupts, and quick requests that should not get lost.",
        accent: "Chat continuity",
    },
    {
        label: "Prepared Turn",
        detail: "A clean handoff object with normalized text, session identity, and source metadata.",
        accent: "Reasoning boundary",
    },
];
export const pillars = [
    {
        title: "Catch the thought early",
        body: "OwnPager holds the request before it disappears.",
    },
    {
        title: "Shape it quietly",
        body: "The system cleans the signal without adding friction.",
    },
    {
        title: "Hand off with context",
        body: "The request reaches the next layer already grounded.",
    },
];
export const audienceNeeds = [
    {
        title: "Send it fast",
        body: "No heavy setup, just capture.",
    },
    {
        title: "Keep the thread",
        body: "Context stays attached to the ask.",
    },
    {
        title: "Trust the handoff",
        body: "Clarity comes before intelligence.",
    },
];
export const workflow = [
    {
        title: "Capture",
        body: "A thought arrives from CLI or Telegram.",
        meta: "Immediate capture",
    },
    {
        title: "Refine",
        body: "OwnPager cleans the signal.",
        meta: "Normalized prompt",
    },
    {
        title: "Preserve",
        body: "Source and session stay attached.",
        meta: "Context preserved",
    },
    {
        title: "Prepare",
        body: "The handoff is ready before reasoning begins.",
        meta: "Prepared turn",
    },
];
export const storyBeats = [
    {
        kicker: "Scene 01",
        title: "A thought appears.",
        body: "OwnPager starts before the user is ready to organize anything.",
        cta: "Catch it early.",
        workspaceLabel: "Capture queue",
        note: "The request is still rough, but it is no longer at risk.",
        messages: [
            {
                role: "user",
                sender: "Founder",
                time: "Now",
                text: "I need to park this idea before I lose it.",
                tone: "user",
            },
            {
                role: "system",
                sender: "OwnPager",
                time: "Captured",
                text: "Captured instantly.",
                tone: "signal",
            },
            {
                role: "system",
                sender: "Session",
                time: "State",
                text: "Urgency becomes structure.",
                tone: "context",
            },
        ],
    },
    {
        kicker: "Scene 02",
        title: "The channel does not matter.",
        body: "CLI and Telegram both lead to the same intake layer.",
        cta: "One surface, many entry points.",
        workspaceLabel: "Channel routing",
        note: "The user should feel continuity, not switching cost.",
        messages: [
            {
                role: "user",
                sender: "CLI",
                time: "Source",
                text: "ownpager cli --message \"Summarize this repo\"",
                tone: "user",
            },
            {
                role: "user",
                sender: "Telegram",
                time: "Source",
                text: "Remember what I wanted to build.",
                tone: "user",
            },
            {
                role: "system",
                sender: "OwnPager",
                time: "Routing",
                text: "Unified intake.",
                tone: "signal",
            },
        ],
    },
    {
        kicker: "Scene 03",
        title: "The signal gets cleaned.",
        body: "OwnPager turns a rough ask into something ready to trust.",
        cta: "Quiet transformation.",
        workspaceLabel: "Normalization pass",
        note: "The request is clearer, but still feels like theirs.",
        messages: [
            {
                role: "system",
                sender: "OwnPager",
                time: "Normalized",
                text: "Signal clarified.",
                tone: "signal",
            },
            {
                role: "system",
                sender: "Metadata",
                time: "Attached",
                text: "Context attached.",
                tone: "context",
            },
            {
                role: "system",
                sender: "Prepared Turn",
                time: "Ready",
                text: "Prepared turn ready.",
                tone: "handoff",
            },
        ],
    },
    {
        kicker: "Scene 04",
        title: "The handoff is clean.",
        body: "The request reaches the next layer already prepared.",
        cta: "Ready for reasoning.",
        workspaceLabel: "Boundary state",
        note: "Nothing dramatic. Just a dependable handoff.",
        messages: [
            {
                role: "system",
                sender: "OwnPager",
                time: "Boundary",
                text: "Prepared turn complete.",
                tone: "handoff",
            },
            {
                role: "system",
                sender: "ThinkingRoot",
                time: "Next",
                text: "Reasoning begins from here.",
                tone: "context",
            },
            {
                role: "system",
                sender: "User trust",
                time: "Result",
                text: "The product already feels dependable.",
                tone: "signal",
            },
        ],
    },
];
export const stats = [
    { value: "2", label: "Entry channels the page highlights" },
    { value: "1", label: "Unified shape behind every request" },
    { value: "0", label: "Backend connections in this prototype" },
];
export const preparedTurnExample = `{
  "request_id": "4ec7c0db-4dc8-42fe-b9dd-8b5a0f85af31",
  "query_text": "Build the bridge",
  "session_key": "telegram:12345:777",
  "workspace_hint": "ownpager",
  "source": {
    "source_type": "telegram",
    "user_id": "777",
    "chat_id": "12345",
    "message_id": "10",
    "cwd": null
  },
  "retrieval_mode": "pre_turn",
  "branch_id": null,
  "user_profile_id": null
}`;
