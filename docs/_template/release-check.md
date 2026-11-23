<!--
Release check scenario template.
Added a feature a user can see (in Discord, the CLI, the installer or the settings tab)? In the same commit
add a scenario for it to the plan's release checklist. Copy the block below, fill it in, delete the comments.
One scenario = one feature, 3–8 steps, written so someone who does not know the code can follow them.
Use a test account and a test server only.
-->

### <Task id> · <Feature name>

**Where:** <Discord client and channel, CLI command, installer screen> · **Client:** <Stable | Canary | Phoenix Canary> · **Language:** <English | Russian | both> · **Needs a second account:** <yes | no>

| # | Step | Expected result |
|:--|:---|:---|
| 1 | <what to click, type or run> | <what should appear: text, button, message> |
| 2 | <next action> | <result> |
| 3 | <wrong input or the plugin turned off> | <a clear message or untouched behaviour, never a crash> |

**Restore after the check:** <plugins to turn off, messages to delete; "nothing">

| Checked by | When | Result |
|:---|:---|:---|
| <who> | DD.MM.YYYY HH:MM | <ok | bug: issue link> |
