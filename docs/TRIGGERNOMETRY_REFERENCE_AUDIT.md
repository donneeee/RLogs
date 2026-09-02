# Triggernometry reference audit for rLogs

Status: source-audited product reference for the menu-only Overlay and Custom
Triggers previews. This document does not enable trigger execution.

Reference: the official
[Triggernometry repository](https://github.com/paissaheavyindustries/Triggernometry),
reviewed from its trigger, condition, folder, action, repository, export, and
editor source rather than from screenshots alone.

## What rLogs should preserve

### Organization and scope

Triggernometry supports nested folders, enabled states, zone restrictions, job
restrictions, event filters, and remote repository content. rLogs should adapt
that into **My Rules & Folders** with scopes for scene, encounter, class,
specialization, role, party state, and personal workflow.

A folder is more than a visual category. Enabling or disabling it must affect
its entire branch, and rules must be able to request a safe enable, disable, or
reset operation on another rule or folder.

### Readable rule construction

Triggernometry separates the event expression, condition tree, action list,
scheduling, debugging, and description. rLogs should preserve those concepts
but present the default editor as one readable sentence:

`When [verified event] -> If [optional conditions] -> Then [ordered actions]`

The event picker should prefer typed, localized BPSR fields such as scene,
source, target, skill, effect, resource, stat, combat state, and timer. Regex,
raw IDs, and expressions belong under **Advanced** and must never replace the
human-readable summary.

### Conditions

Triggernometry condition groups can be nested and use AND, OR, XOR, and NOT.
rLogs should label these in plain language:

- **All** conditions must match;
- **Any** condition may match;
- **Exactly one** condition must match;
- **None** of the conditions may match.

Numeric comparisons, string comparisons, presence checks, list membership,
and advanced regular expressions remain useful. The decision trace must show
the evaluated left value, operation, right value, and result without exposing
private packet contents in a shared pack.

### Timing and action lifecycle

Triggernometry distinguishes sequential action execution, refire suppression,
refire periods, scheduling from the initial fire, the last action, or the
refire period, and whether previously queued actions are kept or interrupted.
rLogs should preserve these under **Timing & Repeat**:

- action delay and ordered versus simultaneous execution;
- cooldown/refire window and duplicate suppression;
- keep, replace, or cancel actions already waiting;
- reset rules at combat, encounter, scene, or manual boundaries;
- explicit cancellation when a rule, folder, encounter, or setup is disabled.

The beginner editor should describe the result, such as “do not run this rule
again for 15 seconds,” instead of exposing scheduler terminology first.

### State and composition

Triggernometry provides scalar, list, table, and dictionary variables, loops,
mutexes, and actions that fire or control other triggers and folders. rLogs
should initially expose a smaller, typed set:

- named text, number, boolean, actor, skill, effect, position, and duration;
- counters, timers, lists, and encounter-scoped state;
- set, add, subtract, append, remove, clear, and compare operations;
- fire, enable, disable, cancel, or reset another rule or folder;
- bounded repeat and for-each operations under Advanced.

State must declare its lifetime: action, rule, combat segment, encounter,
scene, session, or saved setup. This avoids stale state and makes shared rules
predictable.

### Outputs

The useful Triggernometry outputs map to safe rLogs destinations:

| Triggernometry concept | rLogs destination |
| --- | --- |
| text/image aura | Overlay widget or alert |
| sound and text-to-speech | Audio action |
| trigger/folder operation | Custom Triggers state action |
| variable/counter/timer | Rule state or tracker value |
| encounter callout | Alert, countdown, tracker, or map instruction |
| repository update | Versioned trigger-pack update |

The Overlay workspace owns appearance and placement. Custom Triggers may
request that a named overlay target be shown, hidden, updated, or emphasized,
but it does not silently replace that target's visual configuration.

### Testing and diagnosis

Triggernometry includes test input and configurable logging. rLogs should make
this a first-class **Test Lab** rather than burying it in debugging:

- select a recorded event or a clearly marked synthetic sample;
- show whether the event matched;
- show each condition and its evaluated result;
- preview action order, delays, cancellation, and destinations;
- simulate visual/audio actions by default;
- retain a bounded live decision history with an explanation for every skip.

This is essential for both nontechnical users and auditable BPSR behavior.

### Packs, repositories, and sharing

Triggernometry has repositories, manual or startup update policy, local
backups, per-folder and per-trigger states, and explicit permissions for some
powerful actions. rLogs should adapt this into versioned **Trigger Packs** and
**Setup Profiles** with:

- immutable pack ID, author, version, compatible rLogs and BPSR build ranges;
- localized name, description, rule summaries, and update notes;
- declared setup, overlay, sound, map, and rule dependencies;
- a manifest of requested capabilities shown before installation;
- preview and disabled-by-default imports;
- preserved user overrides across safe updates;
- installed version, update status, and rollback metadata;
- export that strips local UID, account, path, and secret data.

The simplest shared-content action remains **Use this setup**. Advanced users
can inspect or selectively install individual folders and rules.

## What rLogs should not copy

Triggernometry supports actions such as arbitrary script execution, process
launching, disk operations, mouse and keyboard injection, window messages,
generic network requests, webhooks, and external application control. Those
are inappropriate defaults for a simple shareable rLogs ecosystem.

Shared rLogs packs must not execute arbitrary code, launch programs, write
arbitrary files, inject input, or contact undeclared network destinations.
Future external integrations, if added, require a separately reviewed and
revocable capability with a visible destination and a narrow schema.

Raw packet bytes and private remote-player evidence also remain outside the
pack format. Rules reference stable, typed event fields and localization keys.

## Approved menu mapping

The Triggernometry-inspired capabilities fit into four top-level tabs:

1. **Overview** — Rules, Library, Event Inspector, and Overlay/Audio/Map
   connections.
2. **Rules** — My Rules & Folders, When, If, Then, Timing & Repeat, State &
   Variables, Test & Review, and Advanced.
3. **Event Inspector** — bounded live follow, decoded details, pin/compare,
   and Create Trigger from a selected event or field.
4. **Library** — Encounter Packs, Timelines, Class & Specialization Packs,
   Imports & Utility, and Installed & Updates.
5. **Settings** — Editor Mode, Rule Execution, Testing, Event Inspector,
   Import & Export,
   Safety & Permissions, and Localization.

This keeps the Triggernometry depth while limiting permanent navigation. A new
user can install a reviewed setup or build one sentence; a power user can open
Advanced inside that same rule.

## Menu approval boundary

Before runtime implementation, confirm:

- Overlay and Custom Triggers remain separate workspaces;
- the five Custom Triggers tabs and six Overlay tabs are understandable;
- Setup Profile and Trigger Pack naming is acceptable;
- Event Inspector belongs in Custom Triggers and remains separate from the
  developer-only Session Tools nested under Settings;
- shared packs are safe and disabled until reviewed;
- advanced controls stay inside each rule instead of becoming more tabs.

Only after menu approval should implementation define the event schema,
runtime scheduler, action registry, overlay bridge, pack format, permissions,
and live testing behavior.
