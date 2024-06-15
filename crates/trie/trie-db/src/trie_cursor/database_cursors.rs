use reth_db::{
    cursor::{DbCursorRO, DbCursorRW, DbDupCursorRO, DbDupCursorRW},
    tables,
    transaction::{DbTx, DbTxMut},
    DatabaseError,
};
use reth_primitives::{
    trie::{
        BranchNodeCompact, Nibbles, StorageTrieEntry, StoredBranchNode, StoredNibbles,
        StoredNibblesSubKey,
    },
    B256,
};
use reth_trie::{
    trie_cursor::{
        DupTrieCursor, DupTrieCursorRw, TrieCursor, TrieCursorErr, TrieCursorFactory, TrieCursorRw,
        TrieCursorRwFactory, TrieCursorWrite,
    },
    updates::TrieKey,
};

/// New-type for a [`DbTx`] and/or [`DbTxMut`] reference with [`TrieCursorFactory`] support.
#[derive(Debug)]
pub struct DbTxRefWrapper<'a, TX>(pub &'a TX);

impl<'a, TX> Clone for DbTxRefWrapper<'a, TX> {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}

/// Converts reference to [`DbTx`] into [`DbTxRefWrapper`].
impl<'a, TX: DbTx> From<&'a TX> for DbTxRefWrapper<'a, TX> {
    fn from(value: &'a TX) -> Self {
        Self(value)
    }
}

/// Implementation of the trie cursor factory for a database transaction.
impl<'a, TX: DbTx> TrieCursorFactory for DbTxRefWrapper<'a, TX> {
    type Err = DatabaseError;

    fn account_trie_cursor(&self) -> Result<Box<dyn TrieCursor<Err = Self::Err> + '_>, Self::Err> {
        Ok(Box::new(DatabaseAccountTrieCursor::new(self.0.cursor_read::<tables::AccountsTrie>()?)))
    }

    fn storage_tries_cursor(
        &self,
        hashed_address: B256,
    ) -> Result<Box<dyn TrieCursor<Err = Self::Err> + '_>, Self::Err> {
        Ok(Box::new(DatabaseStorageTrieCursor::new(
            self.0.cursor_dup_read::<tables::StoragesTrie>()?,
            hashed_address,
        )))
    }
}

/// Implementation of the trie cursor factory for a database transaction.
impl<'a, TX: DbTxMut> TrieCursorRwFactory for DbTxRefWrapper<'a, TX> {
    type Err = DatabaseError;

    fn account_trie_cursor_rw(
        &self,
    ) -> Result<
        Box<dyn TrieCursorRw<StoredNibbles, StoredBranchNode, Err = Self::Err> + '_>,
        Self::Err,
    > {
        self.0.cursor_write::<tables::AccountsTrie>().map(|v| {
            Box::new(DatabaseAccountTrieCursor::new(v))
                as Box<dyn TrieCursorRw<StoredNibbles, StoredBranchNode, Err = Self::Err>>
        })
    }

    fn storage_tries_cursor_rw(
        &self,
    ) -> Result<Box<dyn DupTrieCursorRw<B256, StorageTrieEntry, Err = Self::Err> + '_>, Self::Err>
    {
        self.0.cursor_dup_write::<tables::StoragesTrie>().map(|v| {
            Box::new(DatabaseStoragesTrieCursor::new(v))
                as Box<dyn DupTrieCursorRw<B256, StorageTrieEntry, Err = Self::Err>>
        })
    }
}

/// A cursor over the account trie.
#[derive(Debug)]
pub struct DatabaseAccountTrieCursor<C>(C);

impl<C> DatabaseAccountTrieCursor<C> {
    /// Create a new account trie cursor.
    pub const fn new(cursor: C) -> Self {
        Self(cursor)
    }
}

impl<C> TrieCursorErr for DatabaseAccountTrieCursor<C>
where
    C: Send + Sync,
{
    type Err = DatabaseError;
}

impl<C> TrieCursor for DatabaseAccountTrieCursor<C>
where
    C: DbCursorRO<tables::AccountsTrie> + Send + Sync,
{
    /// Seeks an exact match for the provided key in the account trie.
    fn seek_exact(
        &mut self,
        key: Nibbles,
    ) -> Result<Option<(Nibbles, BranchNodeCompact)>, Self::Err> {
        Ok(self.0.seek_exact(StoredNibbles(key))?.map(|value| (value.0 .0, value.1 .0)))
    }

    /// Seeks a key in the account trie that matches or is greater than the provided key.
    fn seek(&mut self, key: Nibbles) -> Result<Option<(Nibbles, BranchNodeCompact)>, Self::Err> {
        Ok(self.0.seek(StoredNibbles(key))?.map(|value| (value.0 .0, value.1 .0)))
    }

    /// Retrieves the current key in the cursor.
    fn current(&mut self) -> Result<Option<TrieKey>, Self::Err> {
        Ok(self.0.current()?.map(|(k, _)| TrieKey::AccountNode(k)))
    }
}

impl<C> TrieCursorWrite<StoredNibbles, StoredBranchNode> for DatabaseAccountTrieCursor<C>
where
    C: DbCursorRW<tables::AccountsTrie> + Send + Sync,
{
    fn delete_current(&mut self) -> Result<(), Self::Err> {
        self.0.delete_current()
    }

    fn delete_current_duplicates(&mut self) -> Result<(), Self::Err> {
        unimplemented!("Duplicate keys are not supported for accounts trie")
    }

    fn upsert(&mut self, key: StoredNibbles, value: StoredBranchNode) -> Result<(), Self::Err> {
        self.0.upsert(key, value)
    }
}

impl<C> TrieCursorRw<StoredNibbles, StoredBranchNode> for DatabaseAccountTrieCursor<C> where
    C: DbCursorRW<tables::AccountsTrie> + DbCursorRO<tables::AccountsTrie> + Send + Sync
{
}

impl<C> TrieCursorErr for DatabaseStorageTrieCursor<C>
where
    C: Send + Sync,
{
    type Err = DatabaseError;
}

impl<C> TrieCursorWrite<B256, StorageTrieEntry> for DatabaseStorageTrieCursor<C>
where
    C: DbCursorRW<tables::StoragesTrie> + DbDupCursorRW<tables::StoragesTrie> + Send + Sync,
{
    fn delete_current(&mut self) -> Result<(), Self::Err> {
        self.cursor.delete_current()
    }

    fn delete_current_duplicates(&mut self) -> Result<(), Self::Err> {
        self.cursor.delete_current_duplicates()
    }

    fn upsert(&mut self, key: B256, value: StorageTrieEntry) -> Result<(), Self::Err> {
        self.cursor.upsert(key, value)
    }
}

/// A cursor over the storage tries stored in the database.
#[derive(Debug)]
pub struct DatabaseStorageTrieCursor<C> {
    /// The underlying cursor.
    pub cursor: C,
    /// Hashed address used for cursor positioning.
    hashed_address: B256,
}

impl<C> DatabaseStorageTrieCursor<C> {
    /// Create a new storage trie cursor.
    pub const fn new(cursor: C, hashed_address: B256) -> Self {
        Self { cursor, hashed_address }
    }
}

/// A cursor over the storage tries stored in the database.
#[derive(Debug)]
pub struct DatabaseStoragesTrieCursor<C> {
    /// The underlying cursor.
    pub cursor: C,
}

impl<C> DatabaseStoragesTrieCursor<C> {
    /// Create a new storage trie cursor.
    pub const fn new(cursor: C) -> Self {
        Self { cursor }
    }
}

impl<C> TrieCursorErr for DatabaseStoragesTrieCursor<C>
where
    C: Send + Sync,
{
    type Err = DatabaseError;
}

impl<C> TrieCursor for DatabaseStorageTrieCursor<C>
where
    C: DbDupCursorRO<tables::StoragesTrie> + DbCursorRO<tables::StoragesTrie> + Send + Sync,
{
    /// Seeks an exact match for the given key in the storage trie.
    fn seek_exact(
        &mut self,
        key: Nibbles,
    ) -> Result<Option<(Nibbles, BranchNodeCompact)>, Self::Err> {
        Ok(self
            .cursor
            .seek_by_key_subkey(self.hashed_address, StoredNibblesSubKey(key.clone()))?
            .filter(|e| e.nibbles == StoredNibblesSubKey(key))
            .map(|value| (value.nibbles.0, value.node)))
    }

    /// Seeks the given key in the storage trie.
    fn seek(&mut self, key: Nibbles) -> Result<Option<(Nibbles, BranchNodeCompact)>, Self::Err> {
        Ok(self
            .cursor
            .seek_by_key_subkey(self.hashed_address, StoredNibblesSubKey(key))?
            .map(|value| (value.nibbles.0, value.node)))
    }

    /// Retrieves the current value in the storage trie cursor.
    fn current(&mut self) -> Result<Option<TrieKey>, DatabaseError> {
        Ok(self.cursor.current()?.map(|(k, v)| TrieKey::StorageNode(k, v.nibbles)))
    }
}

impl<C> DupTrieCursor<B256> for DatabaseStoragesTrieCursor<C>
where
    C: DbDupCursorRO<tables::StoragesTrie> + DbCursorRO<tables::StoragesTrie> + Send + Sync,
{
    /// Seeks an exact match for the given key in the storage trie.
    fn seek_exact(&mut self, key: B256) -> Result<Option<(Nibbles, BranchNodeCompact)>, Self::Err> {
        Ok(self.cursor.seek_exact(key.clone())?.map(|value| (value.1.nibbles.0, value.1.node)))
    }

    fn seek_by_key_subkey(
        &mut self,
        key: B256,
        subkey: Nibbles,
    ) -> Result<Option<(Nibbles, BranchNodeCompact)>, <Box<Self> as TrieCursorErr>::Err> {
        self.cursor
            .seek_by_key_subkey(key, StoredNibblesSubKey(subkey.clone()))
            .map(|value| value.map(|value| (value.nibbles.0, value.node)))
    }

    /// Retrieves the current value in the storage trie cursor.
    fn current(&mut self) -> Result<Option<TrieKey>, DatabaseError> {
        Ok(self.cursor.current()?.map(|(k, v)| TrieKey::StorageNode(k, v.nibbles)))
    }
}

impl<C> TrieCursorWrite<B256, StorageTrieEntry> for DatabaseStoragesTrieCursor<C>
where
    C: DbCursorRW<tables::StoragesTrie> + DbDupCursorRW<tables::StoragesTrie> + Send + Sync,
{
    fn delete_current(&mut self) -> Result<(), <Box<Self> as TrieCursorErr>::Err> {
        self.cursor.delete_current()
    }

    fn delete_current_duplicates(&mut self) -> Result<(), <Box<Self> as TrieCursorErr>::Err> {
        self.cursor.delete_current_duplicates()
    }

    fn upsert(
        &mut self,
        key: B256,
        value: StorageTrieEntry,
    ) -> Result<(), <Box<Self> as TrieCursorErr>::Err> {
        self.cursor.upsert(key, value)
    }
}

impl<C> DupTrieCursorRw<B256, StorageTrieEntry> for DatabaseStoragesTrieCursor<C> where
    C: DbDupCursorRW<tables::StoragesTrie>
        + DbCursorRW<tables::StoragesTrie>
        + DbDupCursorRO<tables::StoragesTrie>
        + DbCursorRO<tables::StoragesTrie>
        + Send
        + Sync
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth_db::{cursor::DbCursorRW, transaction::DbTxMut};
    use reth_primitives::{
        hex_literal::hex,
        trie::{StorageTrieEntry, StoredBranchNode},
    };
    use reth_provider::test_utils::create_test_provider_factory;

    #[test]
    fn test_account_trie_order() {
        let factory = create_test_provider_factory();
        let provider = factory.provider_rw().unwrap();
        let mut cursor = provider.tx_ref().cursor_write::<tables::AccountsTrie>().unwrap();

        let data = vec![
            hex!("0303040e").to_vec(),
            hex!("030305").to_vec(),
            hex!("03030500").to_vec(),
            hex!("0303050a").to_vec(),
        ];

        for key in data.clone() {
            cursor
                .upsert(
                    key.into(),
                    StoredBranchNode(BranchNodeCompact::new(
                        0b0000_0010_0000_0001,
                        0b0000_0010_0000_0001,
                        0,
                        Vec::default(),
                        None,
                    )),
                )
                .unwrap();
        }

        let db_data = cursor.walk_range(..).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(db_data[0].0 .0.to_vec(), data[0]);
        assert_eq!(db_data[1].0 .0.to_vec(), data[1]);
        assert_eq!(db_data[2].0 .0.to_vec(), data[2]);
        assert_eq!(db_data[3].0 .0.to_vec(), data[3]);

        assert_eq!(
            cursor.seek(hex!("0303040f").to_vec().into()).unwrap().map(|(k, _)| k.0.to_vec()),
            Some(data[1].clone())
        );
    }

    // tests that upsert and seek match on the storage trie cursor
    #[test]
    fn test_storage_cursor_abstraction() {
        let factory = create_test_provider_factory();
        let provider = factory.provider_rw().unwrap();
        let mut cursor = provider.tx_ref().cursor_dup_write::<tables::StoragesTrie>().unwrap();

        let hashed_address = B256::random();
        let key = StoredNibblesSubKey::from(vec![0x2, 0x3]);
        let value = BranchNodeCompact::new(1, 1, 1, vec![B256::random()], None);

        cursor
            .upsert(hashed_address, StorageTrieEntry { nibbles: key.clone(), node: value.clone() })
            .unwrap();

        let mut cursor = DatabaseStorageTrieCursor::new(cursor, hashed_address);
        assert_eq!(cursor.seek(key.into()).unwrap().unwrap().1, value);
    }
}
