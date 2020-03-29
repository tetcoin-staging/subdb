use std::path::PathBuf;
use std::fs::{File, OpenOptions};
use memmap::MmapMut;
use parity_scale_codec::{Encode, Decode};

use crate::datum_size::DatumSize;
use crate::types::{
	KeyType, EntryIndex, TableIndex, INDEX_COUNT, KEY_BYTES, INDEX_BYTES, INDEX_ITEM_SIZE,
	SimpleWriter
};
use crate::index_item::IndexItem;

pub struct SubDb<K> {
	#[allow(dead_code)] path: PathBuf,
	#[allow(dead_code)] index_file: File,
	index: MmapMut,
	//	sized_tables: Vec<Vec<Table<K>>>,
//	oversize_tables: Vec<Option<Table<K>>>,
	_dummy: std::marker::PhantomData<K>,
}

impl<K: KeyType> SubDb<K> {

	#[allow(dead_code)] fn mutate_entry<R>(&mut self, index: usize, f: impl FnOnce(&mut IndexItem) -> R) -> R {
		let data = &mut self.index[index * INDEX_ITEM_SIZE..(index + 1) * INDEX_ITEM_SIZE];
		let mut entry = IndexItem::decode(&mut &data[..]).expect("Database corrupted?!");
		let r = f(&mut entry);
		entry.encode_to(&mut SimpleWriter(data, 0));
		r
	}

	#[allow(dead_code)] fn read_entry(&self, index: usize) -> IndexItem {
		let data = &self.index[index * INDEX_ITEM_SIZE..(index + 1) * INDEX_ITEM_SIZE];
		IndexItem::decode(&mut &data[..]).expect("Database corrupted?!")
	}

	#[allow(dead_code)] fn write_entry(&mut self, index: usize, entry: IndexItem) {
		let data = &mut self.index[index * INDEX_ITEM_SIZE..(index + 1) * INDEX_ITEM_SIZE];
		entry.encode_to(&mut SimpleWriter(data, 0));
	}

	/// Finds the next place to put a piece of data of the given size. Doesn't actually write
	/// anything yet.
	fn find_place(&self, _datum_size: DatumSize) -> (TableIndex, EntryIndex) {
		//TODO

		(0, 0)
	}

	pub fn new(path: PathBuf) -> Self {
		assert!(!path.is_file(), "Path must be a directory or not exist.");
		if !path.is_dir() {
			std::fs::create_dir_all(path.clone()).expect("Path must be writable.");
		}
		let mut index_file_name = path.clone();
		index_file_name.push("index.subdb");
		let index_file = OpenOptions::new()
			.read(true)
			.write(true)
			.create(true)
			.open(&index_file_name)
			.expect("Path must be writable.");
		index_file.set_len((INDEX_COUNT * INDEX_ITEM_SIZE) as u64).expect("Path must be writable.");
		let index = unsafe {
			MmapMut::map_mut(&index_file).expect("Path must be writable.")
		};
		Self { path, index, index_file/*, tables: vec![]*/, _dummy: Default::default() }
	}

	pub fn commit(&mut self) {
		self.index.flush().expect("Flush errored?");
	}

	fn index_of(hash: &K) -> usize {
		hash.as_ref().iter()
			.take(INDEX_BYTES)
			.fold(0, |a, &i| (a << 8) + i as usize)
	}

	// NOTE: the `skipped` flag needs to stick around, even when an item is removed.

	#[allow(dead_code)] pub fn get(&self, hash: &K) -> Option<Vec<u8>> {
		let index = Self::index_of(hash);
		let maybe_entry: Option<IndexItem> = loop {
			let e: IndexItem = self.read_entry(index);
			if !e.is_empty() && &e.key == &hash.as_ref()[0..KEY_BYTES] {
				// Same item (almost certainly) - just need to bump the ref count on the
				// data.
				break Some(e)
			}
			// Check for a past collision...
			if !e.skipped {
				// No collision - item not there.
				return None
			}
		};

		maybe_entry.map(|entry| {
			// TODO: Lookup data from `entry`
			entry.encode()
		})
	}

	#[allow(dead_code)] pub fn put(&mut self, data: &[u8]) -> K {
		let key = K::from_data(data);
		self.put_with_hash(&key, data);
		key
	}

	#[allow(dead_code)] pub fn remove(&mut self, _hash: &K) {
		unimplemented!()
	}

	pub fn put_with_hash(&mut self, hash: &K, data: &[u8]) {
		let index = Self::index_of(hash);

		let datum_size = DatumSize::nearest(data.len());
		let (content_table, entry_index) = self.find_place(datum_size);
		let mut key: [u8; KEY_BYTES] = Default::default();
		key.copy_from_slice(&hash.as_ref()[0..KEY_BYTES]);
		let mut item = IndexItem { key, skipped: false, datum_size, content_table, entry_index };

		let mut final_index = index;
		let already_there = loop {
			match self.mutate_entry(final_index, |e| {
				if !e.is_empty() {
					if &e.key == &item.key {
						// Same item (almost certainly) - just need to bump the ref count on the
						// data.
						item = e.clone();
						return Some(true)
					} else {
						// Collision - highly unlikely, but whatever... flag the item as skipped.
						e.skipped = true;
						return None
					}
				}
				// Nothing there - insert the new item.
				*e = item.clone();
				Some(false)
			}) {
				Some(x) => break(x),
				None => (),
			}
			final_index = (final_index + 1) % INDEX_COUNT;
		};

		if !already_there {
			// TODO: Create the new item at the place in `item` and put data.
		} else {
			// TODO: Bump the refcount of the place in `item`.
		}
	}
}
