FROM rust:1.89

RUN apt-get update
RUN apt-get install sudo qemu-system-x86 llvm libclang-dev hunspell-ru hunspell-en-us texlive-science -y

RUN mkdir -p /opt/shad/nikka
COPY .gitlab-ci.yml /opt/shad/.grader-ci.yml

WORKDIR /opt/shad/nikka
COPY . .

RUN make install
RUN make lint
RUN cargo miri setup

RUN cd sem && cargo build
RUN cd tools && cargo build
RUN cd ku && cargo build
RUN cd kernel && cargo build

RUN grep --files-with-matches --recursive 'compose::begin_private' \
      $(find . -name \*.rs\* -o -name \*.s\*) | \
      cut --fields=2- --delimiter=/ | \
      sort --unique | \
      sed 's@^@  - @' >> .manytask.yml

RUN for task in $(grep task: .manytask.yml | cut -f2 -d: | grep -v 'gdb'); do \
    cargo run --manifest-path tools/check/Cargo.toml -- \
    --student-repo /opt/shad/nikka \
    --original-repo /opt/shad/nikka \
    --ci-branch-name submit/$task \
    --user-id galtsev \
    --no-prerequisites-check \
    --dry-run || exit 1; \
done

RUN cargo run --manifest-path tools/compose/Cargo.toml -- \
    --in-path /opt/shad/nikka \
    --out-path /opt/shad/nikka \
    --spare tools --spare Cargo.lock --spare .compose.yml

RUN if grep -R 'compose::begin_private' $(find . -name \*.rs\* -o -name \*.s\*); then exit 1; fi
