# Notion: AI fills every column, fixed emoji per database

Date: 2026-11-23. Status: draft for approval.

## Why

The user sends tasks from meetings (German lessons, work calls) to Notion databases with more columns than Conclave fills today (e.g. *Die Hausaufgaben AI*: Hausaufgabe, Datum, Thema, Status, Notizen). Rows arrive half empty. The user wants the AI to fill every column from the meeting itself, the way a person would, without deciding the values per column. Each database also gets a fixed emoji as the page icon (e.g. 👷).

## Goals

- When the user clicks Send, every writable column of the target database gets a value chosen by the AI from the meeting's summary and transcript.
- A fixed emoji per database is set as the icon of every page Conclave creates there.

## Non-goals

- No preview or editing of AI-filled values before sending (approach C, possible later).
- No new select/multi-select/status options: the AI picks only from options that already exist.
- No people, relation, file, formula, rollup or created/edited columns.

## Anti-goals

- A failed or bad AI answer must never block a send, cause a duplicate page, or write a value Notion rejects.
- No silent failure: if the AI could not fill the columns, the user is told.

## Approaches

- **A. AI fills at Send (chosen).** One AI call per database per send, with the tasks, meeting context and the database's columns and options. Works for any database, including ones added later and tasks whose database was picked in the review queue. Costs one cheap call; the values are seen in Notion, not before.
- B. AI fills at extraction. No extra call, but extraction happens before the database is known, so it only works with a category default and breaks when the database changes. Rejected.
- C. AI fills when the task gets a database, shown in the review queue. Most control, but a much larger review-screen change, and the user wants to delegate. Kept as a later option.

## Design

- **Settings → Notion, per database:**
  - an optional **Emoji** field, set as the page `icon` on every new page;
  - a **"Let AI fill the other columns"** switch, default on (builder's call: default on, because this is what the user asked for; the switch lets a database opt out).
- **What the AI fills:** every column of kind date, rich_text, select, multi_select, status, checkbox, number or url that is not already the title, not mapped (due date, meeting) and not given a fixed value. Mapped and fixed columns keep winning.
- **AI input:**
  - the tasks being sent (text, owner, due);
  - the meeting title, date and summary;
  - the transcript, capped to a few thousand words to keep the cost low (builder sets the cap);
  - today's date;
  - the columns with their kind and existing options.
- **The call:** the same LLM provider, model and key the user set for summaries.
- **Output check (trust boundary):** the answer is a JSON value per task per column. Each value is checked against the schema read from Notion: an option that doesn't exist is dropped, a wrong type is dropped, and a bad date is dropped. Only checked values go into `page_body`.
- **Failure:** if the AI call fails or returns nothing usable, the send goes ahead with the mapped and fixed values only, and the send result tells the user "AI couldn't fill the other columns" (and logs why). The AI call happens before any page is created, so the existing "never sent twice" and "may already be in Notion" logic is unchanged.

## Left to the builder

- The prompt wording, the transcript cap, and how tasks from several meetings in one send are grouped (one call per meeting per database is fine).
- Where the AI call lives (a new module next to `notion/send.rs`, or in `knowledge/`).
- The storage of the two new per-database settings: they extend the existing `mapping_json`, with `#[serde(default)]` so saved databases keep working.

## Constraints

- No new network destinations beyond the user's LLM provider and Notion (`docs/network-calls.md`).
- Gates per AGENTS.md; the Notion send tests stay green.
