package main

import (
	"context"

	"go.einride.tech/sage/sg"
	"go.einride.tech/sage/tools/sgconvco"
	"go.einride.tech/sage/tools/sggit"
	"go.einride.tech/sage/tools/sgmarkdownfmt"
	"go.einride.tech/sage/tools/sgyamlfmt"
)

func main() {
	sg.GenerateMakefiles(
		sg.Makefile{
			Path:          sg.FromGitRoot("Makefile"),
			DefaultTarget: Default,
		},
	)
}

func Default(ctx context.Context) error {
	sg.Deps(ctx, ConvcoCheck, FormatMarkdown, FormatYaml)
	sg.Deps(ctx, Fmt, Lint)
	sg.Deps(ctx, Test, Build)
	sg.Deps(ctx, GitVerifyNoDiff)
	return nil
}

func Test(ctx context.Context) error {
	sg.Logger(ctx).Println("running Rust tests...")
	cmd := sg.Command(ctx, "cargo", "test", "--all")
	cmd.Env = append(
		cmd.Env,
		"RUST_BACKTRACE=1",
	)
	return cmd.Run()
}

func Run(ctx context.Context) error {
	sg.Logger(ctx).Println("Formatting Rust files...")
	return sg.Command(ctx, "cargo", "run").Run()
}

func Build(ctx context.Context) error {
	sg.Logger(ctx).Println("Formatting Rust files...")
	return sg.Command(ctx, "cargo", "build").Run()
}

func Fmt(ctx context.Context) error {
	sg.Logger(ctx).Println("Formatting Rust files...")
	return sg.Command(ctx, "cargo", "fmt", "--all", "--", "--check").Run()
}

func Lint(ctx context.Context) error {
	sg.Logger(ctx).Println("linting Rust files...")
	return sg.Command(ctx, "cargo", "clippy", "--all", "--", "-D", "warnings").Run()
}

func FormatMarkdown(ctx context.Context) error {
	sg.Logger(ctx).Println("formatting Markdown files...")
	return sgmarkdownfmt.Command(ctx, "-w", ".").Run()
}

func FormatYaml(ctx context.Context) error {
	sg.Logger(ctx).Println("formatting Yaml files...")
	return sgyamlfmt.Run(ctx)
}

func ConvcoCheck(ctx context.Context) error {
	sg.Logger(ctx).Println("checking git commits...")
	return sgconvco.Command(ctx, "check", "origin/master..HEAD").Run()
}

func GitVerifyNoDiff(ctx context.Context) error {
	sg.Logger(ctx).Println("verifying that git has no diff...")
	return sggit.VerifyNoDiff(ctx)
}
