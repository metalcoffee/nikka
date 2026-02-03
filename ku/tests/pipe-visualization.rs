#![deny(warnings)]
#![feature(allocator_api)]

use std::{
    fs,
    ops::Range,
    vec,
};

use rstest::rstest;

use ku::{
    error::{
        Error::NoData,
        Result,
    },
    ipc::pipe::{
        self,
        RingBufferStats,
        test_scaffolding::{
            CLEAR,
            CLOSED,
            READ,
            WRITTEN,
            block,
            head_tail,
            header_size,
            stats,
            tx_head_tail,
        },
    },
    log::error,
    memory::{
        Block,
        Page,
    },
};

use allocator::BigForPipe;

mod allocator;
mod log;

#[rstest]
fn make_diagrams() {
    continuous_ring_buffer().unwrap();
    pipe_usage_example().unwrap();
    tx_example().unwrap();
}

fn continuous_ring_buffer() -> Result<()> {
    let buffer_len = 16;

    let mut buffer = vec![BigForPipe::GARBAGE; buffer_len];
    let header_size = 3;
    let payload = b"continuous";
    let payload_start = 3;
    buffer[payload_start .. payload_start + payload.len()].copy_from_slice(payload);
    let continuous = Diagram::from_slice(&buffer, header_size)
        .y(25.0)
        .cell_size(9.0)
        .payload_style(r##"\large\bf{}"##);

    let mut discontinuous_buffer = vec![BigForPipe::GARBAGE; buffer_len];
    let payload = b"discontinuous";
    let payload_start = 8;
    discontinuous_buffer[payload_start ..].copy_from_slice(&payload[.. buffer_len - payload_start]);
    discontinuous_buffer[.. payload.len() - (buffer_len - payload_start)]
        .copy_from_slice(&payload[buffer_len - payload_start ..]);
    let discontinuous = Diagram::from_slice(&discontinuous_buffer, header_size)
        .cell_size(9.0)
        .payload_style(r##"\Large\bf{}"##);

    DiagramSet::new()
        .push(continuous)
        .push(discontinuous)
        .write("../lab/src/6-um-1-pipe-discontinuous-payload.tex");

    let mut continuous_buffer = vec![BigForPipe::GARBAGE; 2 * buffer_len];
    let payload = b"discontinuous";
    let payload_start = 8;
    for shift in [0, buffer_len] {
        continuous_buffer[shift + payload_start .. shift + buffer_len]
            .copy_from_slice(&payload[.. buffer_len - payload_start]);
        continuous_buffer[shift .. shift + payload.len() - (buffer_len - payload_start)]
            .copy_from_slice(&payload[buffer_len - payload_start ..]);
    }
    let phys_last = buffer_len - 1;
    let virt_last = 2 * buffer_len - 1;
    let virt = Diagram::from_slice(&continuous_buffer, header_size)
        .cell_size(9.0)
        .payload_style(r##"\Large\bf{}"##)
        .node("virt")
        .y(90.0)
        .brace_above(
            payload_start,
            payload_start + payload.len(),
            "continuous view of the payload",
        )
        .brace_below(0, buffer_len, "the first virtual page set")
        .brace_below(
            buffer_len,
            2 * buffer_len,
            r##"
            \begin{tabular}{c}
                the second virtual page set \\
                mapped to the same frames
            \end{tabular}
            "##,
        );
    let phys = Diagram::from_slice(&discontinuous_buffer, header_size)
        .cell_size(9.0)
        .payload_style(r##"\Large\bf{}"##)
        .node("phys")
        .brace_below(0, buffer_len, "physical frames");
    DiagramSet::new()
        .push(virt)
        .push(phys)
        .edge("virt0.south west", "phys0.north west")
        .edge(&format!("virt{buffer_len}.south west"), "phys0.north west")
        .edge(
            &format!("virt{phys_last}.south east"),
            &format!("phys{phys_last}.north east"),
        )
        .edge(
            &format!("virt{virt_last}.south east"),
            &format!("phys{phys_last}.north east"),
        )
        .write("../lab/src/6-um-1-pipe-continuous-payload.tex");

    Ok(())
}

fn pipe_usage_example() -> Result<()> {
    let range = 0 .. 28;

    let mut allocator = BigForPipe::new(false);

    // ANCHOR: make
    let (mut read_buffer, mut write_buffer) = pipe::make(PIPE_FRAME_COUNT, &mut allocator)?;
    // ANCHOR_END: make
    let buffer = block(&write_buffer);
    let header_size = header_size(&write_buffer);
    let (head, tail) = head_tail(&write_buffer);
    Diagram::new(buffer, header_size, range.clone())
        .legend(Legend::Compact)
        .head_tail(head, tail, &["read", "and", "write"])
        .write("../lab/src/6-um-1-pipe-0-make-pipe.tex");

    // ANCHOR: write_tx_0
    let mut write_tx_0 = write_buffer.write_tx().ok_or(NoData)?;
    // ANCHOR_END: write_tx_0
    let (head, tail) = tx_head_tail(&write_tx_0);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["write tx"])
        .write("../lab/src/6-um-1-pipe-1-write-tx-0.tex");

    // ANCHOR: write_0
    write_tx_0.write(b"Lin")?;
    // ANCHOR_END: write_0
    let (head, tail) = tx_head_tail(&write_tx_0);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["write tx"])
        .write("../lab/src/6-um-1-pipe-2-write-0.tex");

    // ANCHOR: write_tx_0_drop
    drop(write_tx_0);
    // ANCHOR_END: write_tx_0_drop
    unsafe {
        buffer.try_into_mut_slice::<u8>()?[head + header_size .. tail].fill(BigForPipe::GARBAGE);
    };
    let (head, tail) = head_tail(&write_buffer);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &[])
        .write("../lab/src/6-um-1-pipe-3-write-tx-0-drop.tex");

    // ANCHOR: write_tx_1
    let mut write_tx_1 = write_buffer.write_tx().ok_or(NoData)?;
    // ANCHOR_END: write_tx_1
    let (head, tail) = tx_head_tail(&write_tx_1);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["write tx"])
        .write("../lab/src/6-um-1-pipe-4-write-tx-1.tex");

    // ANCHOR: write_1
    write_tx_1.write(b"Nik")?;
    // ANCHOR_END: write_1
    let (head, tail) = tx_head_tail(&write_tx_1);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["write tx"])
        .write("../lab/src/6-um-1-pipe-5-write-1.tex");

    // ANCHOR: write_2
    write_tx_1.write(b"ka")?;
    // ANCHOR_END: write_2
    let (head, tail) = tx_head_tail(&write_tx_1);
    let (write_tx_1_head, write_tx_1_tail) = (head, tail);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["write tx"])
        .write("../lab/src/6-um-1-pipe-6-write-2.tex");

    // ANCHOR: read_tx_0
    let mut read_tx_0 = read_buffer.read_tx().ok_or(NoData)?;
    unsafe {
        assert_eq!(read_tx_0.read(), None);
    }
    // ANCHOR_END: read_tx_0
    let (head, tail) = tx_head_tail(&read_tx_0);
    // ANCHOR: read_tx_0_drop
    drop(read_tx_0);
    // ANCHOR_END: read_tx_0_drop
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["read tx"])
        .write("../lab/src/6-um-1-pipe-7-read-tx-0.tex");

    let (head, tail) = tx_head_tail(&write_tx_1);
    // ANCHOR: write_tx_1_commit
    write_tx_1.commit();
    // ANCHOR_END: write_tx_1_commit
    Diagram::new(buffer, header_size, range.clone())
        .legend(Legend::Verbose)
        .head_tail(head, tail, &["write tx"])
        .brace_above(
            write_tx_1_head,
            write_tx_1_tail,
            r##"Write transaction \#1 --- one record"##,
        )
        .write("../lab/src/6-um-1-pipe-8-write-tx-1-commit.tex");

    // ANCHOR: write_tx_2
    let mut write_tx_2 = write_buffer.write_tx().ok_or(NoData)?;
    write_tx_2.write(b" ")?;
    // ANCHOR_END: write_tx_2
    let (head, tail) = tx_head_tail(&write_tx_2);
    let (write_tx_2_head, write_tx_2_tail) = (write_tx_1_tail, tail);
    // ANCHOR: write_tx_2_commit
    write_tx_2.commit();
    // ANCHOR_END: write_tx_2_commit
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["write tx"])
        .brace_above(write_tx_1_head, write_tx_1_tail, r##"Write tx \#1"##)
        .brace_above(write_tx_2_head, write_tx_2_tail, r##"Write tx \#2"##)
        .write("../lab/src/6-um-1-pipe-9-write-tx-2.tex");

    let (head, tail) = head_tail(&read_buffer);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["read"])
        .stats(stats(&read_buffer), stats(&write_buffer))
        .write("../lab/src/6-um-1-pipe-10-read-buffer.tex");

    // ANCHOR: read_tx_1
    let mut read_tx_1 = read_buffer.read_tx().ok_or(NoData)?;
    // ANCHOR_END: read_tx_1
    let (head, tail) = tx_head_tail(&read_tx_1);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["read tx"])
        .write("../lab/src/6-um-1-pipe-11-read-tx-1.tex");

    // ANCHOR: read_1
    unsafe {
        assert_eq!(read_tx_1.read().ok_or(NoData)?, b"Nikka");
    }
    // ANCHOR_END: read_1
    let (head, tail) = tx_head_tail(&read_tx_1);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["read tx"])
        .write("../lab/src/6-um-1-pipe-12-read-1.tex");

    // ANCHOR: read_2
    unsafe {
        assert_eq!(read_tx_1.read().ok_or(NoData)?, b" ");
    }
    // ANCHOR_END: read_2
    let (head, tail) = tx_head_tail(&read_tx_1);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["read tx"])
        .write("../lab/src/6-um-1-pipe-13-read-2.tex");

    // ANCHOR: read_3
    unsafe {
        assert_eq!(read_tx_1.read(), None);
    }
    // ANCHOR_END: read_3

    // ANCHOR: write_tx_3
    let mut write_tx_3 = write_buffer.write_tx().ok_or(NoData)?;
    write_tx_3.write(b"rocks")?;
    // ANCHOR_END: write_tx_3
    let (head, tail) = tx_head_tail(&write_tx_3);
    let (write_tx_3_head, write_tx_3_tail) = (write_tx_2_tail, tail);
    // ANCHOR: write_tx_3_commit
    write_tx_3.commit();
    // ANCHOR_END: write_tx_3_commit
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["write tx"])
        .brace_above(write_tx_1_head, write_tx_1_tail, r##"Write tx \#1"##)
        .brace_above(write_tx_2_head, write_tx_2_tail, r##"Write tx \#2"##)
        .brace_above(write_tx_3_head, write_tx_3_tail, r##"Write tx \#3"##)
        .write("../lab/src/6-um-1-pipe-14-write-tx-3.tex");

    // ANCHOR: write_tx_4
    let mut write_tx_4 = write_buffer.write_tx().ok_or(NoData)?;
    write_tx_4.write(b"!")?;
    // ANCHOR_END: write_tx_4
    let (head, tail) = tx_head_tail(&write_tx_4);
    let (write_tx_4_head, write_tx_4_tail) = (write_tx_3_tail, tail);
    // ANCHOR: write_tx_4_commit
    write_tx_4.commit();
    // ANCHOR_END: write_tx_4_commit
    Diagram::new(buffer, header_size, range.clone())
        .legend(Legend::Normal)
        .head_tail(head, tail, &["write tx"])
        .brace_above(write_tx_1_head, write_tx_1_tail, r##"Write tx \#1"##)
        .brace_above(write_tx_2_head, write_tx_2_tail, r##"Write tx \#2"##)
        .brace_above(write_tx_3_head, write_tx_3_tail, r##"Write tx \#3"##)
        .brace_above(write_tx_4_head, write_tx_4_tail, r##"Write tx \#4"##)
        .write("../lab/src/6-um-1-pipe-15-write-tx-4.tex");

    // ANCHOR: read_4
    unsafe {
        assert_eq!(read_tx_1.read().ok_or(NoData)?, b"rocks");
    }
    // ANCHOR_END: read_4
    let (head, tail) = tx_head_tail(&read_tx_1);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["read tx"])
        .write("../lab/src/6-um-1-pipe-16-read-4.tex");

    // ANCHOR: read_tx_1_drop
    drop(read_tx_1);
    // ANCHOR_END: read_tx_1_drop
    let (head, tail) = head_tail(&read_buffer);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["read"])
        .stats(stats(&read_buffer), stats(&write_buffer))
        .write("../lab/src/6-um-1-pipe-17-read-tx-1-drop.tex");

    // ANCHOR: read_tx_2
    let mut read_tx_2 = read_buffer.read_tx().ok_or(NoData)?;
    // ANCHOR_END: read_tx_2
    let (read_tx_2_head, _) = tx_head_tail(&read_tx_2);
    // ANCHOR: read_tx_2_reads
    unsafe {
        assert_eq!(read_tx_2.read().ok_or(NoData)?, b"Nikka");
    }
    unsafe {
        assert_eq!(read_tx_2.read().ok_or(NoData)?, b" ");
    }
    unsafe {
        assert_eq!(read_tx_2.read().ok_or(NoData)?, b"rocks");
    }
    // ANCHOR_END: read_tx_2_reads
    let (head, tail) = tx_head_tail(&read_tx_2);
    let read_tx_2_tail = head;
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["read tx"])
        .write("../lab/src/6-um-1-pipe-18-read-tx-2.tex");

    unsafe {
        buffer.try_into_mut_slice::<u8>()?[read_tx_2_head + header_size .. read_tx_2_tail]
            .fill(BigForPipe::GARBAGE);
    };
    // ANCHOR: read_tx_2_commit
    read_tx_2.commit();
    // ANCHOR_END: read_tx_2_commit
    let (head, tail) = head_tail(&read_buffer);
    Diagram::new(buffer, header_size, range.clone())
        .legend(Legend::Verbose)
        .head_tail(head, tail, &["read tx"])
        .brace_above(
            read_tx_2_head,
            read_tx_2_tail,
            "Read transaction --- several records replaced by one",
        )
        .write("../lab/src/6-um-1-pipe-19-read-tx-commit.tex");

    // ANCHOR: write_tx_5
    let mut write_tx_5 = write_buffer.write_tx().ok_or(NoData)?;
    let capacity = write_tx_5.capacity();
    for _ in 0 .. capacity {
        write_tx_5.write(b"*")?;
    }
    // ANCHOR_END: write_tx_5
    let (head, tail) = tx_head_tail(&write_tx_5);
    // ANCHOR: write_tx_5_commit
    write_tx_5.commit();
    // ANCHOR_END: write_tx_5_commit
    Diagram::new(buffer, header_size, range.clone())
        .legend(Legend::Verbose)
        .head_tail(head, tail, &["write tx"])
        .write("../lab/src/6-um-1-pipe-20-write-tx-5.tex");

    // ANCHOR: read_tx_3
    let mut read_tx_3 = read_buffer.read_tx().ok_or(NoData)?;
    // ANCHOR_END: read_tx_3
    let (head, tail) = tx_head_tail(&read_tx_3);
    // ANCHOR: read_tx_3_commit
    unsafe {
        assert_eq!(read_tx_3.read().ok_or(NoData)?, b"!");
    }
    unsafe {
        assert_eq!(read_tx_3.read().ok_or(NoData)?.len(), capacity);
    }
    read_tx_3.commit();
    // ANCHOR_END: read_tx_3_commit
    unsafe {
        buffer.try_into_mut_slice::<u8>()?[head + header_size .. range.end]
            .fill(BigForPipe::GARBAGE);
        buffer.try_into_mut_slice::<u8>()?[.. tail].fill(BigForPipe::GARBAGE);
    };
    let (head, tail) = head_tail(&read_buffer);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["read tx"])
        .write("../lab/src/6-um-1-pipe-21-read-tx-3.tex");

    // ANCHOR: read_close
    read_buffer.close();
    // ANCHOR_END: read_close
    let (head, tail) = head_tail(&read_buffer);
    Diagram::new(buffer, header_size, range.clone())
        .head_tail(head, tail, &["read", "and", "write"])
        .stats(stats(&read_buffer), stats(&write_buffer))
        .write("../lab/src/6-um-1-pipe-22-read-buffer-close.tex");

    // ANCHOR: no_more_txs
    assert!(write_buffer.write_tx().is_none());
    assert!(read_buffer.read_tx().is_none());
    // ANCHOR_END: no_more_txs

    allocator.unmap();

    Ok(())
}

fn tx_example() -> Result<()> {
    let mut allocator = BigForPipe::new(false);

    // ANCHOR: tx_example
    let (_, mut write_buffer) = pipe::make(PIPE_FRAME_COUNT, &mut allocator)?;
    let header_size = header_size(&write_buffer);
    let mut write_tx = write_buffer.write_tx().ok_or(NoData)?;
    write_tx.write(b"Nikka rocks!")?;
    write_tx.commit();
    // ANCHOR_END: tx_example

    let buffer = block(&write_buffer);
    let (head, tail) = head_tail(&write_buffer);
    let payload_size = tail - head - header_size;

    Diagram::new(buffer, header_size, head .. tail)
        .cell_size(17.0)
        .header_style(r##"\Large\bf{}0x"##)
        .payload_style(r##"\Large\bf{}"##)
        .legend(Legend::Verbose)
        .brace_above(head, head + header_size, "header")
        .brace_above(
            head + header_size,
            tail,
            &format!("payload ({payload_size} bytes)"),
        )
        .brace_below(head, head + 1, "state")
        .brace_below(
            head + 1,
            head + header_size,
            &format!(
                r##"
                \begin{{tabular}}{{c}}
                    payload size\\0x{payload_size:04X} = {payload_size}
                \end{{tabular}}
                "##,
            ),
        )
        .write("../lab/src/6-um-1-pipe-tx-header.tex");

    allocator.unmap();

    Ok(())
}

#[must_use]
struct Diagram<'a> {
    buffer: &'a [u8],
    cell_size: f32,
    contents: String,
    header_size: usize,
    header_style: String,
    legend: Legend,
    node: String,
    payload_style: String,
    range: Range<usize>,
    y: f32,
}

impl<'a> Diagram<'a> {
    fn new(
        buffer: Block<Page>,
        header_size: usize,
        range: Range<usize>,
    ) -> Self {
        let buffer = unsafe { buffer.try_into_slice::<u8>().unwrap() };
        Self::from_slice(&buffer[range.start .. range.end], header_size)
    }

    fn from_slice(
        buffer: &'a [u8],
        header_size: usize,
    ) -> Self {
        Self {
            buffer,
            cell_size: 7.0,
            contents: String::new(),
            header_size,
            header_style: r##"\bf{}"##.into(),
            legend: Legend::None,
            node: "node".into(),
            payload_style: r##"\bf{}"##.into(),
            range: 0 .. buffer.len(),
            y: 0.0,
        }
    }

    fn brace_above(
        mut self,
        head: usize,
        tail: usize,
        label: &str,
    ) -> Self {
        let node = &self.node;
        let last = tail - 1;
        self.contents += &format!(
            r##"
            \draw[decorate, decoration = {{calligraphic brace, raise=5pt, amplitude=10pt}}]
                ({node}{head}.130) -- ({node}{last}.50)
                node[midway, above=6mm] {{\Large\bf{{}}{label}\strut}};"##,
        );
        self
    }

    fn brace_below(
        mut self,
        head: usize,
        tail: usize,
        label: &str,
    ) -> Self {
        let node = &self.node;
        let last = tail - 1;
        self.contents += &format!(
            r##"
            \draw[decorate, decoration = {{calligraphic brace, mirror, raise=5pt, amplitude=10pt}}]
                ({node}{head}.230) -- ({node}{last}.310)
                node[midway, below=6mm] {{\Large\bf{{}}{label}\strut}};"##,
        );
        self
    }

    fn cell_size(
        mut self,
        mm: f32,
    ) -> Self {
        self.cell_size = mm;
        self
    }

    fn head_tail(
        mut self,
        head: usize,
        tail: usize,
        tags: &[&str],
    ) -> Self {
        let base_on_head = tail < self.range.end - 1;
        let node = &self.node;

        let head_position = if too_close(head, tail) && !base_on_head {
            "left=2mm of tail".into()
        } else {
            below(&format!("{node}{head}"))
        };
        let tail_position = if too_close(head, tail) && base_on_head {
            if head <= tail {
                "right=2mm of head"
            } else {
                "left=2mm of head"
            }
            .into()
        } else {
            below(&format!("{node}{tail}"))
        };

        let mut label = "".to_string();
        for tag in tags {
            label += &format!(r##"\large\bf{{{tag}}} \\"##);
        }
        let head_node = mark("head", &head_position, &label);
        let tail_node = mark("tail", &tail_position, &label);

        self.contents += &(if base_on_head {
            head_node + &tail_node
        } else {
            tail_node + &head_node
        });

        self.contents += &Self::edge("head.north", &format!("{node}{head}.south west"));
        self.contents += &Self::edge("tail.north", &format!("{node}{tail}.south west"));

        return self;

        fn below(node: &str) -> String {
            format!("below=20mm of {node}.west")
        }

        fn mark(
            node: &str,
            position: &str,
            tag: &str,
        ) -> String {
            format!(
                r##"
                \node[draw, shape=chamfered rectangle, {position}, minimum size=10mm] ({node}) {{
                    \begin{{tabular}}{{c}} {tag} \large\bf{{{node}}} \\
                    \end{{tabular}}
                }};"##,
            )
        }

        fn too_close(
            head: usize,
            tail: usize,
        ) -> bool {
            tail.abs_diff(head) < 4
        }
    }

    fn header_style(
        mut self,
        header_style: &str,
    ) -> Self {
        self.header_style = header_style.into();
        self
    }

    fn legend(
        mut self,
        legend: Legend,
    ) -> Self {
        self.legend = legend;
        self
    }

    fn node(
        mut self,
        node: &str,
    ) -> Self {
        self.node = node.into();
        self
    }

    fn stats(
        mut self,
        read: RingBufferStats,
        write: RingBufferStats,
    ) -> Self {
        let node = &self.node;
        let last = self.range.end - 1;
        self.contents += &format!(
            r##"
            \matrix[
                stats,
                above=12mm of {node}{last}.north east,
                anchor=south east,
                text width=6em
            ] {{"##,
        );
        self.contents += r##"
            {\Large\bf{}Stats} & committed & commits & dropped & drops & errors & txs \\"##;

        for (title, stats) in [("read", &read), ("write", &write)] {
            self.contents += &format!(
                r##"
                {} & {} & {} & {} & {} & {} & {} \\"##,
                title,
                stats.committed(),
                stats.commits(),
                stats.dropped(),
                stats.drops(),
                stats.errors(),
                stats.txs(),
            );
        }

        self.contents += "\n            };\n";

        self
    }

    fn payload_style(
        mut self,
        payload_style: &str,
    ) -> Self {
        self.payload_style = payload_style.into();
        self
    }

    fn y(
        mut self,
        y: f32,
    ) -> Self {
        self.y = y;
        self
    }

    fn write(
        self,
        file: &str,
    ) {
        DiagramSet::new().push(self).write(file);
    }

    fn edge(
        from: &str,
        to: &str,
    ) -> String {
        format!(
            r##"
            \draw[-{{Latex[length=5mm, width=2mm]}}] ({from}) -- ({to});"##,
        )
    }

    fn format_buffer(&self) -> String {
        let mut closed = false;
        let mut fill = "";
        let mut fill_active = 0;

        let start = self.range.start;

        let mut result = String::new();

        for i in self.range.clone() {
            let mut contents = format!(r##"{}{:02X}\strut"##, self.header_style, self.buffer[i]);
            let position = i - start;

            if fill_active == 0 {
                fill = match self.buffer[i] {
                    BigForPipe::GARBAGE => {
                        contents = "".into();
                        "garbage"
                    },
                    CLEAR => "headerclear",
                    CLOSED => {
                        closed = true;
                        "headerclosed"
                    },
                    WRITTEN => {
                        fill_active = self.header_size - 1;
                        "headerwritten"
                    },
                    READ => {
                        fill_active = self.header_size - 1;
                        "headerread"
                    },
                    _ => {
                        contents = format!(
                            r##"{}{}\strut"##,
                            self.payload_style, self.buffer[i] as char
                        );
                        "payload"
                    },
                };
            } else {
                fill_active -= 1;
            }

            if closed && fill != "headerclosed" {
                contents = "".into();
                fill = "garbage";
            }

            let cell_size = self.cell_size;
            let node = &self.node;
            let position = position as f32 * self.cell_size;
            let y = self.y;
            result += &format!(
                r##"
                \node[draw, fill={fill}, minimum size={cell_size}mm] at
                    ({position}mm, {y}mm) ({node}{i}) {{ {contents} }};"##,
            );
        }

        result
    }

    fn format_legend(&self) -> String {
        if self.legend == Legend::None {
            return "".into();
        }

        let mut size = "size".into();
        if self.legend >= Legend::Verbose {
            size += r##"\&{}0xFF"##;
            for i in 1 .. self.header_size - 1 {
                let shift = i * 8;
                size = format!(r##"(size\textgreater\textgreater{shift}{{}})\&{{}}0xFF, {size}"##);
            }
        } else {
            size += r##"\ldots"##;
        }

        let width = match self.legend {
            Legend::None => "",
            Legend::Compact => "35mm",
            Legend::Normal => "70mm",
            Legend::Verbose => "115mm",
        };

        let mut clear = "".into();
        let mut closed = "".into();
        let mut read = "".into();
        let mut written = "".into();

        if self.legend >= Legend::Normal {
            clear = format!(" = [0x{CLEAR:02X}]");
            closed = format!(" = [0x{CLOSED:02X}]");
            read = format!(" = [0x{READ:02X}, {size}]");
            written = format!(" = [0x{WRITTEN:02X}, {size}]");
        }

        format!(
            r##"
            \matrix[
                above=15mm of {}{}.north east,
                anchor=south east,
                column sep=0.5mm,
                row sep=1mm
            ] (legend) {{
                &&
                \node[draw, fill=headerclear, text width={}, align=left] {{
                    \large\bf\strut Header::Clear{}
                }}; \\
                &&
                \node[draw, fill=headerwritten, text width={}, align=left] {{
                    \large\bf\strut Header::Written{}
                }}; \\
                \node[draw, fill=payload, text width=35mm, align=left] {{
                    \large\bf\strut payload
                }}; &&
                \node[draw, fill=headerread, text width={}, align=left] {{
                    \large\bf\strut Header::Read{}
                }}; \\
                \node[draw, fill=garbage, text width=35mm, align=left] {{
                    \large\bf\strut garbage
                }}; &&
                \node[draw, fill=headerclosed, text width={}, align=left] {{
                    \large\bf\strut Header::Closed{}
                }}; \\
            }};
            \node[
                below=2mm of legend.north west,
                anchor=north west,
                text width=30mm,
                align=left
            ] {{ \Huge Legend: }};"##,
            self.node,
            self.range.end - 1,
            width,
            clear,
            width,
            written,
            width,
            read,
            width,
            closed
        )
    }
}

struct DiagramSet<'a> {
    contents: String,
    diagrams: Vec<Diagram<'a>>,
}

impl<'a> DiagramSet<'a> {
    fn new() -> Self {
        Self {
            contents: String::new(),
            diagrams: Vec::new(),
        }
    }

    fn edge(
        mut self,
        from: &str,
        to: &str,
    ) -> Self {
        self.contents += &Diagram::edge(from, to);
        self
    }

    fn push(
        mut self,
        diagram: Diagram<'a>,
    ) -> Self {
        self.diagrams.push(diagram);
        self
    }

    fn write(
        self,
        file: &str,
    ) {
        let mut contents = HEADER.to_string();

        for diagram in self.diagrams.into_iter() {
            contents = contents +
                &diagram.format_buffer() +
                &diagram.contents +
                &diagram.format_legend() +
                "\n";
        }

        contents = contents + &self.contents + TAILER;

        if let Err(error) = fs::write(file, contents) {
            error!(%error, file, "failed to write the file");
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Legend {
    None,
    Compact,
    Normal,
    Verbose,
}

#[ctor::ctor]
fn init() {
    log::init();
}

const PIPE_FRAME_COUNT: usize = 4;

const HEADER: &str = r##"
\documentclass[border=2mm]{standalone}

\usepackage[dvipsnames]{xcolor}
\usepackage{pgfmath,pgffor}
\usepackage{tikz}

\usetikzlibrary{arrows.meta}
\usetikzlibrary{decorations.pathreplacing,calligraphy}
\usetikzlibrary{math}
\usetikzlibrary{matrix}
\usetikzlibrary{positioning}
\usetikzlibrary{shapes.misc}

\begin{document}

\definecolor{headerclear}{RGB}{202, 218, 246}
\definecolor{headerwritten}{RGB}{221, 253, 234}
\definecolor{headerread}{RGB}{252, 244, 221}
\definecolor{headerclosed}{RGB}{252, 225, 228}
\definecolor{garbage}{RGB}{204, 204, 204}
\definecolor{payload}{RGB}{255, 255, 255}

\begin{tikzpicture}[ultra thick]

\tikzset{
    stats/.style={
        matrix of nodes,
        row sep=-\pgflinewidth,
        column sep=-\pgflinewidth,
        nodes={rectangle, draw=black, align=left, font=\bf},
        minimum height=2em,
        text depth=0.5ex,
        text height=2ex,
        nodes in empty cells,
        column 1/.style={nodes={fill=headerclear, font=\large\bf}},
        row 1/.style={nodes={fill=headerclear, font=\large\bf}},
    }
}

"##;

const TAILER: &str = r##"
\end{tikzpicture}

\end{document}
"##;
