# Spec: tray-notifications

## ADDED Requirements

### Requirement: Menubar widget
The app SHALL provide a menubar/tray item showing the next upcoming event's attending count and days-until. Clicking it SHALL open a compact popover with the next event's funnel and a link to the full detail view.

#### Scenario: Tray reflects next event
- **WHEN** the cache holds an upcoming event with 94 attending starting in 3 days
- **THEN** the tray shows a compact representation of "94 · 3d" and the popover names the event

#### Scenario: No upcoming events
- **WHEN** no upcoming events exist in cache
- **THEN** the tray shows an idle state and the popover offers to open the main window

### Requirement: RSVP change notifications
The app SHALL send a native OS notification when a poll cycle detects a change in attending or waitlisted counts for an upcoming event, including the event name and old → new count. At most one notification per event per poll cycle SHALL be sent.

#### Scenario: Count increase
- **WHEN** a poll cycle detects attending change from 91 to 94 for an upcoming event
- **THEN** a notification like "AI Tinkerers Boston: attending 91 → 94" is shown once

#### Scenario: Notification preferences
- **WHEN** the user disables notifications in settings
- **THEN** poll-diff changes update the tray and UI but no OS notifications are sent

### Requirement: First-sync suppression
Notifications SHALL be suppressed for the initial population of the cache (no prior value to diff against) and after sign-out/sign-in.

#### Scenario: Fresh install
- **WHEN** the first successful sync populates event counts
- **THEN** no notifications fire for that cycle
