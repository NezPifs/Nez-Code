# Nez-Code

Nez-Code is a local-first coding agent for the terminal. It runs against local
models through [Ollama](https://ollama.com) by default and can also use hosted
OpenAI-compatible providers such as DeepSeek.

> Full install, usage and configuration documentation lands with the 3.0.0
> release. Until then, build from source:

```sh
cd nez-rs
cargo build --release --bin nez
./target/release/nez
```

## License

Nez-Code is licensed under the [Apache License 2.0](LICENSE). It is based on
and modified from [openai/codex](https://github.com/openai/codex); see
[NOTICE](NOTICE).
