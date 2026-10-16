# Decisions waiting for the founder

The most important ones come first. Each one has a recommendation. Nothing changes until you say yes.

1. **Rename the product.**
   - Why: there is a US trademark (PUPPETMASTER, reg. 5987353), Puppet, Inc. owns "PUPPET" marks for developer software, an open-source project `puppetmaster-ai` has the same pitch, and the name sounds like an insult in Mandarin (Taiwan).
   - Recommendation: rename to **Ninedot** before the alpha. Other names: Monocrew, Kelpie, Dotcrew, Troupe. Talk to a trademark lawyer.
   - Details: `D:\pm-night\NAME-BRAND.md`.
2. **Code-signing certificate.** Buy one, so Windows stops saying "unknown publisher".
3. **Licence.**
   - Recommendation: the app and the protocol are MIT. The relay server is AGPL-3.0 (open, and anyone can host it). Accounts and billing stay private.
4. **Agent authority defaults.**
   - Switch A, "Ask me before an agent controls a pane it didn't start". Today it is off. Recommendation: **on**.
   - Switch B, "Let agents merge separate copies". Today it is on. Recommendation: **off**.
5. **Default rights for a newly paired phone.** Today a new phone gets full control. Recommendation: **watch + answer**, which is safer if the phone is lost.
6. **Move the Orchestration/Individual switch.** Recommendation: move it out of the sidebar, into the Ctrl+K palette and Settings, because one wrong click surprises people.
7. **Merge PR #3** (0.2.1 into `main`): https://github.com/Potato-dev-inc/puppet-master/pull/3
8. **Warn on old release pages.** Add a line to 0.1.0–0.1.4 saying "has a security hole, update". Recommendation: yes.
9. **Ask for a CVE number** for the security advisory. Optional.
10. **Website.**
    - Final prices: $8 / $16 are shown as "Planned".
    - Connect the waitlist form.
    - Remove `noindex` at launch.
    - See [05-website.md](05-website.md).
11. **Windows first?** Recommendation: yes. Sell and support only Windows at launch.
12. **Keep "Cursor" in the headline?** Recommendation: only after Cursor passes the trust-screen test.
13. **Privacy of usage numbers.** Recommendation: opt-in only, plus a privacy page.
14. **Things only you can do.**
    - Ask Anthropic and OpenAI to confirm in writing that running their unmodified programs with your own login is fine.
    - Choose a merchant of record and talk to an accountant.

## Already decided for you (you said "you decide"). You can change any of these.

- **Codex live updates (hooks) are opt-in**, because they write a small file into each project.
- **Missions ships as Preview, off by default.** It needs a budget and the tamper guard, and runs at most 2 missions at once.
- **Hand-off at the limit is detect-only first.** At most 3 hand-offs, no automatic hand-back, and Cursor is excluded.
- **The OpenCode automatic key switch is opt-in (off).**
- **"Stop all agents" skips plain shells**, so dev servers keep running.
- **Separate copy is on by default** for git projects that have at least one commit.
- **A declined run's empty copy** is removed when you close its pane.
- **Folder-trust cards** always say "Trust this folder / Don't trust", and need a real click.
