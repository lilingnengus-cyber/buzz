# LifeOS resource link acceptance

Status: completed on 2026-09-06 (Asia/Shanghai).

## Delivered behavior

Verified LifeOS replies render their resource references as clickable Markdown
autolinks. Clicking an action or project reference opens the corresponding
native LifeOS interface in the Pacioli Life Dock. Clicking a project reference
also reopens a closed Dock and navigates to that project.

The first action check reached an embed resource summary. The user correctly
rejected that as insufficient: matching a resource title and ID does not prove
that the native detail interface opened. The subsequent correction maps action
resources to `/actions?action=<id>` and selects the existing ActionPreviewDrawer.
It preserves native controls and opens completed actions despite the default
list status filter. Legacy embed action routes redirect to that native route.
Project resources already map to `/projects/<id>`.

## Implementation and release

- Pacioli commit `69c5e99a8`: verified reply references become clickable autolinks.
  The local release binary was installed and the existing LifeOS agent restarted;
  its log confirmed online presence at 2026-09-05T18:01:07Z.
- LifeOS commit `c27d6bdbb9e0abf9a75ded7aea9b995af790bc8f`: native action routing,
  URL selection and missing/inaccessible action feedback.
- LifeOS production deployment `33983061726` succeeded (2m4s).

## Validation

Automated checks passed: 10 Rust reply formatter tests, 87 Markdown tests,
3 Life Dock Playwright smoke tests, desktop typecheck/build and affected Biome
checks. Native routing checks passed: 7 Life bridge tests, 5 embed route checks,
and LifeOS TypeScript checking.

Live validation used `/Applications/Pacioli.app`, the managed
`助理Agent_LifeOS`, and the production LifeOS deployment. The requests were
explicitly read-only; no business data was changed during these acceptance runs.

1. Queried one existing action. The new reply contained a clickable reference
   `life://action/cmowme4q80005wmpzh0rmtvnx`.
   Trace: `c2f514d5-25ab-43e3-b3fc-f7a9c6db47b7`.
   Audit: `f1770a00-149c-4413-885a-1e35785b2203`.
   After the native routing fix, navigating to the Dock home and clicking this
   same reply link opened `/actions?action=cmowme4q80005wmpzh0rmtvnx`.
   The native drawer displayed the matching action title, project, priority,
   estimated duration, completion information, parent/child controls, edit entry,
   Pomodoro controls and AI execution history. The user confirmed acceptance.
2. Queried one existing project. The reply contained `life://project/p1` for
   `生存系统V1`.
   Trace: `8496d71f-188d-4786-9de9-06ff7ee8c7ed`.
   Audit: `becab5b6-fa06-4a07-aa4e-21032d4f0ec1`.
   From the Dock home, clicking the reply link opened `/projects/p1` and showed
   the native project war room, action area and project knowledge context.
3. Closed the Dock (toggle confirmed off), clicked the same project reply link,
   and verified the Dock reopened (toggle on) at `/projects/p1`, with matching
   project name and native detail sections. The page initially showed its retained
   project list while navigation completed; the final route and content were
   explicitly checked.

## Scope and limits

This acceptance covers native action/project navigation and reopening a closed
Dock for a project. Historical reply text is unchanged. Browser-modifier behavior,
other resource types and expired-session recovery were not newly live-tested in
this acceptance. The earlier action creation authorization issue is a separate
work item; these read-only link checks do not establish its resolution.
