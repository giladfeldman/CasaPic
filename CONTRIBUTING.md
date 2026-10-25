# Contributing to CasaPic

Thank you for helping. Bug reports, ideas and pull requests are all welcome.

## Reporting a bug

Open an issue and include:
- what you did, what you expected, and what happened instead;
- your Windows version, and the photo's file type if the problem is with one photo;
- a screenshot if it helps.

Please do not attach personal photos. A photo that shows the problem and that you are happy to
share is fine.

## Changing the code

1. Fork the repository and make a branch.
2. Build and run it as described in the README. While you work, point CasaPic at a test folder
   with `CASAPIC_ROOTS` and `CASAPIC_DATA` so it never touches your own library.
3. Before you open a pull request, run:

   ```powershell
   cargo test --workspace
   npm run check
   ```

   Both must pass.
4. Keep each pull request to one change, and say in it how you checked that the change works.

## Ground rules for the code

- **Never change a photo file without being asked.** Edits live in `.casapic.ini` recipes. A
  file is written only by Save, Adjust Date, tags or captions, and its untouched original is
  kept first in `.casapicoriginals`.
- **No network access.** CasaPic works entirely offline and sends nothing anywhere.
- **No Picasa code or files.** CasaPic is written from scratch. Do not copy code, images or
  other material from Picasa or any other program whose licence does not allow it.
- Every edit tool is a recipe operation in `crates/core/src/recipe.rs`, drawn in
  `crates/core/src/render.rs` (or a module it calls), with a test in the same crate.

## Licence

By contributing you agree that your contribution is released under the project's licence,
the GNU General Public License version 3 or later.
