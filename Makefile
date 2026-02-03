QEMU_OPTIONS = -cpu qemu64 -m size=50M -smp cpus=4 -device isa-debug-exit,iobase=0xF4,iosize=0x04
QEMU_GTK = $(QEMU_OPTIONS) -display gtk -serial file:serial.out
QEMU_NOX = $(QEMU_OPTIONS) -nographic -serial mon:stdio
QEMU_CURSES = $(QEMU_OPTIONS) -display curses -serial file:serial.out

QUIT_MESSAGE = "To quit QEMU type Ctrl-A then X. For QEMU console type Ctrl-A then C.\n"
WAIT_MESSAGE = "Will wait for GDB to attach. Run 'make gdb' in a separate console after qemu starts.\n"

BUILD_MODE = debug

ifeq ($(BUILD_MODE),debug)
    BUILD_MODE_FLAG =
else
    BUILD_MODE_FLAG = --release
endif

RUST_PATH = $(HOME)/.cargo/bin

export PATH := $(RUST_PATH):$(PATH)

CRATES = ku pci pic8259 serial text kernel sem
WORKSPACES = . sem

all: test run

run: install
	@( \
		cd kernel; \
		rm --force serial.out; \
		cargo run $(BUILD_MODE_FLAG) -- $(QEMU_GTK); \
	)

run-gdb: install
	@echo $(WAIT_MESSAGE)
	@echo
	@( \
		cd kernel; \
		rm --force serial.out; \
		cargo run $(BUILD_MODE_FLAG) -- $(QEMU_GTK) -gdb tcp::1234 -S; \
	)

nox: install
	@echo $(QUIT_MESSAGE)
	@echo
	@( \
		cd kernel; \
		rm --force serial.out; \
		cargo run $(BUILD_MODE_FLAG) -- $(QEMU_NOX); \
	)

nox-gdb: install
	@echo $(QUIT_MESSAGE)
	@echo $(WAIT_MESSAGE)
	@echo
	@( \
		cd kernel; \
		rm --force serial.out; \
		cargo run $(BUILD_MODE_FLAG) -- $(QEMU_NOX) -gdb tcp::1234 -S; \
	)

curses: install
	@( \
		cd kernel; \
		rm --force serial.out; \
		cargo run $(BUILD_MODE_FLAG) -- $(QEMU_CURSES); \
	)

kill:
	@kill $$(ps -u $$USER -o pid,comm | awk '$$2 == "qemu-system-x86" { print $$1; }')

gdb:
	gdb -command=gdbinit

FORCE:

lint: install
	@( \
		rustup component add clippy; \
		rustup component add rustfmt; \
		nikka_root=$$(pwd); \
		for crate in $(CRATES); do \
			cd $$crate; \
			spellcheck=$$(cargo spellcheck --cfg $$nikka_root/tools/lint/spellcheck.toml | cat --squeeze-blank); \
			if [ $$(echo -n "$$l" | wc -c) -gt 0 ]; then \
				echo "Spelling errors found."; \
				echo "$$spellcheck"; \
				exit 1; \
			fi; \
			typos --config $$nikka_root/tools/lint/typos.toml || exit 1; \
			cargo fmt --check || exit 1; \
			cargo clippy --all-targets || exit 1; \
			cd - >/dev/null; \
		done; \
		taplo format --config tools/lint/taplo.toml --check $$(find . -name \*.toml) || exit 1; \
		long_lines=$$(grep '^.\{101\}' $$(find . -name \*.rs | grep -v -e '^\./target/' -e '^\./sem/target/' -e 'tests/with_sentinel_frame/expand') | grep -v -e '^[^:]*: *///' -e 'compose::public' -e 'https*://'); \
		if [ ! -z "$$long_lines" ]; then \
			echo "Too long lines found."; \
			echo "$$long_lines"; \
			exit 1; \
		fi; \
		typos=$$(hunspell -d ru_RU,en_US -p tools/lint/spellcheck.exceptions -l lab/src/*.md sem/*/*.md); \
		if [ ! -z "$$typos" ]; then \
			echo "Typos found."; \
			echo "$$typos"; \
			exit 1; \
		fi; \
	)

test: install
	@( \
		rustup component add miri; \
		for crate in $(CRATES); do \
			cd $$crate; \
			if [ .$$crate = .ku ]; then \
				test_flags='--profile ci -- --test-threads=1'; \
			elif [ .$$crate = .kernel ]; then \
				test_flags='--profile ci --features forbid-leaks'; \
			else \
				test_flags=''; \
			fi; \
			RUST_BACKTRACE=1 cargo test $$test_flags || exit 1; \
			cd - >/dev/null; \
		done; \
		cd ku; \
		for test in 1-time-2-once-lock 1-time-5-correlation-point; do \
			MIRIFLAGS="-Zmiri-disable-isolation" cargo miri test --test $$test; \
		done; \
		cargo test --profile benchmark --test 3-allocator-5-small-memory-allocator --features benchmark -- --test-threads=1; \
		cd - >/dev/null; \
		cd sem/rwlock; \
		MIRIFLAGS="-Zmiri-disable-isolation" cargo miri test -- --nocapture --test-threads=1; \
		cd - >/dev/null; \
	)

test-gdb: install
	@( \
		for crate in kernel; do \
			cd $$crate; \
			RUST_BACKTRACE=1 cargo test -- -gdb tcp::1234 -S; \
			cd - >/dev/null; \
		done; \
	)

lab: $(RUST_PATH)/mdbook doc lab/src/*.dot lab/src/*.md lab/src/*.puml
	@( \
		if grep --quiet 'compose::begin_private' ku/src/sync/pipe.rs; then \
			cd ku; \
			cargo test --test pipe-visualization; \
			cd - >/dev/null; \
			cd lab/src; \
			sed '/(node0)/{s@headerwritten@headerclear@g;s@01@00@g;}' < 6-um-1-pipe-8-write-tx-1-commit.tex > 6-um-1-pipe-8-write-tx-1-commit-1-part.tex; \
			sed '/(node0)/{s@headerread@headerwritten@g;s@02@01@g;}' < 6-um-1-pipe-19-read-tx-commit.tex > 6-um-1-pipe-19-read-tx-commit-1-part.tex; \
			cd - >/dev/null; \
		fi; \
		sed -i 's@/home/[a-z-]*/tmp/[a-z-]*/@/.../nikka/@g;s@/home/[a-z-]*/[a-z-]*/@/.../nikka/@g;s@/home/[a-z-]*/\.@/.../.@g' lab/src/*.md; \
		cargo run --manifest-path tools/check/Cargo.toml -- --student-repo . --original-repo . --ci-branch-name submit/- --user-id - --dump-dependencies > lab/src/0-intro-1-nikka-lab-dependencies.dot; \
		for i in 1-hw "2-memory 2-mm" 3-allocator 4-process 5-sync "6-user-memory-tricks 6-um" "7-file-system 7-fs"; do \
			lab_dot=$$(echo "$$i $$i"); \
			lab=$$(echo "$$lab_dot" | cut -f1 -d' '); \
			dot=$$(echo "$$lab_dot" | cut -f2 -d' '); \
			cargo run --manifest-path tools/check/Cargo.toml -- \
				--student-repo . \
				--original-repo . \
				--ci-branch-name submit/- \
				--user-id - \
				--dump-group-dependencies lab-$${lab} > lab/src/$${dot}-0-intro-lab-dependencies.dot; \
		done; \
		cd lab/src; \
		for i in *.dot; do dot -Tsvg $$i > $${i%dot}svg; done; \
		for i in *.puml; do java -jar /usr/share/java/plantuml/plantuml.jar -tsvg $$i; done; \
		for i in 3-allocator-7-*.tex 6-um-1-pipe*.tex; do \
			sed -i 's@\s*$$@@g;' $$i; \
			pdflatex -halt-on-error $$i >/dev/null; \
			latex_error=$$?; \
			cat $${i%tex}log | grep --after-context=2 '^!'; \
			if [ $$latex_error -ne 0 ]; then exit $$latex_error; fi; \
			pdf2svg $${i%tex}pdf $${i%tex}svg; \
		done; \
		cd - >/dev/null; \
		cd lab; \
		cp book-base.toml book.toml; \
		mdbook build; \
		cp book-linkcheck.toml book.toml; \
		mdbook build; \
		mv book/html html; \
		rm --force --recursive book book.toml; \
		mkdir book; \
		mv html book/; \
		mv book/html/* book/; \
		cd - >/dev/null; \
		rm --force lab/src/*.aux lab/src/*.log lab/src/*.pdf; \
	)

fs-dump:
	@xxd fs.img | sed 's@.*0000 0000 0000 0000 0000 0000 0000 0000.*@...@' | uniq

doc: FORCE
	@( \
		cargo doc --document-private-items --features allocator-statistics; \
		rm --force $$(grep --files-with-matches 'compose::begin_private' $$(find target/doc/src -name \*.rs.html) ); \
		rm --force --recursive doc; \
		mv target/doc .; \
	)

clean:
	@( \
		for crate in $(CRATES) tools; do \
			cd $$crate; \
			cargo clean; \
			cd - >/dev/null; \
		done; \
	)
	@rm --force --recursive \
		doc \
		fs.img \
		kernel/serial.out \
		ku/*.aux ku/*.log ku/*.pdf ku/*.tex \
		lab/book \
		lab/src/*.aux lab/src/*.log lab/src/*.pdf lab/src/*.svg \
		lab/src/*-lab-dependencies.dot \
		$(find . -name \*.rs.bk)

stat:
	@cargo run --manifest-path tools/compose/Cargo.toml -- --in-path . --stat

compose:
	@cargo run --manifest-path tools/compose/Cargo.toml -- --in-path . --out-path .

install:
	@(which rustup > /dev/null || (curl https://sh.rustup.rs -sSf | sh))
	@(which bootimage > /dev/null || (cargo install bootimage))
	@(which rustfilt > /dev/null || (cargo install rustfilt))
	@(which cargo-expand > /dev/null || (cargo install cargo-expand))
	@(which cargo-spellcheck  > /dev/null || (cargo install cargo-spellcheck))
	@(which typos > /dev/null || (cargo install typos-cli))
	@(which taplo > /dev/null || (cargo install taplo-cli))

$(RUST_PATH)/mdbook:
	cargo install mdbook
	cargo install mdbook-mermaid
	cargo install mdbook-plantuml

nikka.pdf: FORCE
	@trueprint \
		--no-page-break-after-function \
		--point-size=16 \
		--no-holepunch \
		--no-top-holepunch \
		--single-sided \
		--one-up \
		--no-cover-sheet \
		--function-index \
		--landscape \
		--no-headers \
		--output=/dev/stdout \
		$$(jj file list | grep '\.rs$$' | sort --unique) | \
		ps2pdf /dev/stdin $@

pdf: FORCE
	rm -rf pdf
	mkdir pdf
	for i in $$(jj file list | grep '\.rs$$' | sort --unique); do \
		j=$$(echo $$i | sed 's@^./@@;s@/@,@g'); \
		trueprint \
		--no-page-break-after-function \
		--point-size=16 \
		--no-holepunch \
		--no-top-holepunch \
		--single-sided \
		--one-up \
		--no-cover-sheet \
		--function-index \
		--landscape \
		--no-headers \
		--output=/dev/stdout \
		$$i | \
		ps2pdf /dev/stdin pdf/$$j.pdf; \
	done

nikka.enscript.pdf: FORCE
	enscript -1rE -C -H -j -O --toc -b '$n|20$D $C|$% / $= ; $p' -T 2 -f 'Courier@12' -M A5 $$(find . -name '*.rs' | sort --unique) -o nikka.enscript.pdf

src.pdf: doc FORCE
	rm -rf doc/src.pdf
	cp -pr doc/src doc/src.pdf
	cargo doc --document-private-items --no-default-features
	rm --force --recursive doc
	mv target/doc .
	for i in $$(find doc/src.pdf '*.html'); do \
		echo "-------------------------------------- "$i \
		sed -i '/^<span id="[0-9]*">[0-9]*<\/span>/d;s@<body class="rustdoc source">.*@<body>@' $$i; \
		j=$$(echo $$i | sed 's@\.html@.pdf@'); \
		wkhtmltopdf \
			--enable-local-file-access \
			--minimum-font-size 32 \
			--orientation Landscape \
			--page-size A5 \
			$$i $$j; \
		rm -f $$i; \
	done
	mv doc/src.pdf .
