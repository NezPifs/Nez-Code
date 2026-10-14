<p align="center"><strong>Nez-Code</strong> is a coding agent from OpenAI that runs locally on your computer.
<p align="center">
  <img src="https://github.com/NezPifs/Nez-Code/blob/main/.github/nez-cli-splash.png" alt="Nez-Code splash" width="80%" />
</p>
</br>
If you want Nez in your code editor (VS Code, Cursor, Windsurf), <a href="https://developers.openai.com/nez/ide">install in your IDE.</a>
</br>If you want the desktop app experience, run <code>nez app</code> or visit <a href="https://chatgpt.com/nez?app-landing-page=true">the Nez App page</a>.
</br>If you are looking for the <em>cloud-based agent</em> from OpenAI, <strong>Nez Web</strong>, go to <a href="https://chatgpt.com/nez">chatgpt.com/nez</a>.</p>

---

## Quickstart

### Installing and running Nez-Code

Run the following on Mac or Linux to install Nez-Code:

```shell
curl -fsSL https://chatgpt.com/nez/install.sh | sh
```

Run the following on Windows to install Nez-Code:

```shell
powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/nez/install.ps1 | iex"
```

The standalone installers download from `https://releases.openai.com/nez` by default and fall back to GitHub Releases if a metadata or asset download is unavailable. To force GitHub Releases, set `NEZ_INSTALLER_USE_RELEASES_OPENAI_COM` to `false` (`0` and `no` are also accepted):

```shell
curl -fsSL https://chatgpt.com/nez/install.sh | NEZ_INSTALLER_USE_RELEASES_OPENAI_COM=false sh
```

```powershell
$env:NEZ_INSTALLER_USE_RELEASES_OPENAI_COM='false'; irm https://chatgpt.com/nez/install.ps1 | iex
```

Nez-Code can also be installed via the following package managers:

```shell
# Install using npm
npm install -g @openai/nez
```

```shell
# Install using Homebrew
brew install --cask nez
```

Then simply run `nez` to get started.

<details>
<summary>You can also go to the <a href="https://github.com/NezPifs/Nez-Code/releases/latest">latest GitHub Release</a> and download the appropriate binary for your platform.</summary>

Each GitHub Release contains many executables, but in practice, you likely want one of these:

- macOS
  - Apple Silicon/arm64: `nez-aarch64-apple-darwin.tar.gz`
  - x86_64 (older Mac hardware): `nez-x86_64-apple-darwin.tar.gz`
- Linux
  - x86_64: `nez-x86_64-unknown-linux-musl.tar.gz`
  - arm64: `nez-aarch64-unknown-linux-musl.tar.gz`

Each archive contains a single entry with the platform baked into the name (e.g., `nez-x86_64-unknown-linux-musl`), so you likely want to rename it to `nez` after extracting it.

</details>

### Using Nez with your ChatGPT plan

Run `nez` and select **Sign in with ChatGPT**. We recommend signing into your ChatGPT account to use Nez as part of your Plus, Pro, Business, Edu, or Enterprise plan. [Learn more about what's included in your ChatGPT plan](https://help.openai.com/en/articles/11369540-nez-in-chatgpt).

You can also use Nez with an API key, but this requires [additional setup](https://developers.openai.com/nez/auth#sign-in-with-an-api-key).

## Docs

- [**Nez Documentation**](https://developers.openai.com/nez)
- [**Contributing**](./docs/contributing.md)
- [**Installing & building**](./docs/install.md)
- [**Open source fund**](./docs/open-source-fund.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
