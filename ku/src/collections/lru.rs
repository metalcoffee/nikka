use alloc::{
    collections::BTreeMap,
    vec::Vec,
};
use core::{
    borrow::Borrow,
    cmp::Ord,
    fmt,
};

/// Реализация политики вытеснения давно неиспользуемых данных
/// ([Least Recently Used (LRU)](https://en.wikipedia.org/wiki/Cache_replacement_policies#LRU)).
///
/// Предполагает, что ключ `K` и значение `V` --- легковесные типы.
/// А сам кэш хранит своё содержимое где-то в другом месте.
#[derive(Clone, Debug)]
pub struct Lru<K, V>
where
    K: Clone + Ord,
    V: Clone,
{
    /// Отображение ключей в индексы, по которым хранятся узлы со значениями.
    /// То есть, значения [`Lru::map`] --- это индексы в [`Lru::nodes`].
    map: BTreeMap<K, usize>,

    /// Хранилище для узлов, отображающих ключи в значения.
    /// Эти же узлы провязаны в LRU--очередь на основе двусвязного списка.
    /// LRU--очередь перечисляет узлы в порядке их последнего использования,
    /// от дольше всего не использовавшегося до использованного последним.
    nodes: Vec<Node<K, V>>,

    /// Голова LRU--очереди --- узел, который не использовался дольше всего.
    head: Option<usize>,

    /// Хвост LRU--очереди --- узел, который использовался последним.
    tail: Option<usize>,

    /// Статистика работы кэша.
    stats: Stats,
}

#[derive(Clone, Debug)]
/// Узел LRU--очереди.
///
/// LRU--очередь перечисляет узлы в порядке их последнего использования,
/// от дольше всего не использовавшегося до использованного последним.
struct Node<K, V> {
    /// Ключ.
    key: K,

    /// Значение.
    value: V,

    /// Предыдущий узел, он использовался последний раз перед последним использованием текущего.
    /// Хранится как индекс в [`Lru::nodes`].
    prev: Option<usize>,

    /// Следующий узел, он использовался последний раз после последнего использования текущего.
    /// Хранится как индекс в [`Lru::nodes`].
    next: Option<usize>,
}

impl<K, V> Lru<K, V>
where
    K: Clone + Ord,
    V: Clone,
{
    /// Создаёт LRU--кэш с ограничением на ёмкость `capacity`.
    ///
    /// # Panics
    ///
    /// Паникует, если ограничение на ёмкость нулевое.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);

        Self {
            nodes: Vec::with_capacity(capacity),
            map: BTreeMap::new(),
            head: None,
            tail: None,
            stats: Stats::default(),
        }
    }

    /// Статистика работы кэша.
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// Сохраняет в кэш заданную пару ключ--значение.
    /// Обновляет время доступа к записи, если она есть.
    /// Возвращает:
    ///   - Пару ключ--значение, которую при этом пришлось вытеснить из кэша,
    ///     если до операции уже было достигнуто ограничение на его текущую ёмкость.
    ///   - [`None`], если ограничение на текущую ёмкость кэша
    ///     не было достигнуто на момент начала операции.
    pub fn insert(
        &mut self,
        key: K,
        value: V,
    ) -> Option<(K, V)> {
        // TODO: your code here.
        None // TODO: remove before flight.
    }

    /// Возвращает значение для заданного ключа `key`,
    /// или [`None`], если соответствующей записи нет.
    /// Обновляет время доступа к записи, если она есть.
    pub fn get<Q>(
        &mut self,
        key: &Q,
    ) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        // TODO: your code here.
        None // TODO: remove before flight.
    }

    /// Удаляет из кэша запись с заданным ключом `key`, если она есть.
    /// Возвращает:
    ///   - Значение для удалённого ключа.
    ///   - [`None`], если по ключу `key` в кэше ничего не найдено.
    pub fn remove<Q>(
        &mut self,
        key: &Q,
    ) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: Ord,
    {
        // TODO: your code here.
        None // TODO: remove before flight.
    }

    /// Удаляет из кэша запись, которая не обновлялась дольше всех остальных.
    /// Возвращает удалённую пару ключ--значение.
    fn evict(&mut self) -> Option<(K, V)> {
        // TODO: your code here.
        unimplemented!();
    }

    /// Удаляет из кэша запись по её индексу `id` в [`Lru::nodes`].
    /// Возвращает удалённую пару ключ--значение.
    fn remove_node(
        &mut self,
        id: usize,
    ) -> Option<(K, V)> {
        // TODO: your code here.
        unimplemented!();
    }

    /// Устанавливает в `next` ссылку [`Node::next`] узла,
    /// который предшествует узлу номер `id` в структуре очереди,
    /// либо обновляет [`Lru::head`], если предшествующего узла нет.
    fn set_next_in_prev(
        &mut self,
        id: usize,
        next: Option<usize>,
    ) {
        // TODO: your code here.
        unimplemented!();
    }

    /// Устанавливает в `prev` ссылку [`Node::prev`] узла,
    /// который следует за узлом номер `id` в структуре очереди,
    /// либо обновляет [`Lru::tail`], если следующего узла нет.
    fn set_prev_in_next(
        &mut self,
        id: usize,
        prev: Option<usize>,
    ) {
        // TODO: your code here.
        unimplemented!();
    }

    /// Проверяет внутренние инварианты LRU--кэша
    ///
    /// # Panics
    ///
    /// Паникует, если инварианты нарушены.
    pub fn validate(&self) {
        let mut curr = self.head;
        let mut prev = None;
        let mut tail = None;
        let mut count = 0;

        assert_eq!(self.nodes.len(), self.map.len());
        assert!(self.nodes.len() <= self.nodes.capacity());

        while let Some(id) = curr {
            assert!(count < self.nodes.len());
            assert!(id < self.nodes.len());

            let node = &self.nodes[id];

            assert_eq!(node.prev, prev);
            assert_eq!(self.map.get(&node.key), Some(&id));

            prev = curr;
            curr = node.next;
            if curr.is_none() {
                tail = Some(id);
            }

            count += 1;
        }

        assert_eq!(self.tail, tail);
    }
}

impl<K, V> fmt::Display for Lru<K, V>
where
    K: Clone + Ord + fmt::Display,
    V: Clone + fmt::Display,
{
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(
            formatter,
            "head: {:?}, tail: {:?}, lru: [",
            self.head, self.tail,
        )?;

        let mut separator = "";
        let mut curr = self.head;

        while let Some(id) = curr {
            let node = &self.nodes[id];

            write!(
                formatter,
                "{}{{id: {}, key: {}, value: {}, prev: {:?}, next: {:?}}}",
                separator, id, node.key, node.value, node.prev, node.next,
            )?;

            curr = node.next;
            separator = ", ";
        }

        write!(formatter, "], map: {{")?;

        separator = "";
        for (key, id) in self.map.iter() {
            let value = &self.nodes[*id].value;
            write!(formatter, "{separator}{key} -> {value} [{id}]")?;
            separator = ", ";
        }

        write!(formatter, "}}")
    }
}

/// Статистика работы кэша.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Количество вытеснений.
    evictions: usize,

    /// Количество попаданий в кэш.
    hits: usize,

    /// Количество промахов мимо кэша.
    misses: usize,
}
