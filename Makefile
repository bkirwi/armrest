data/inks/iam-ondb-trainset.txt: data/iam-ondb script/training.py
	script/training.py ondb_to_text data/iam-ondb/ $@ --subset trainset

data/inks/iam-ondb-validset.txt: data/iam-ondb script/training.py
	script/training.py ondb_to_text data/iam-ondb/ $@ --subset testset_v

data/inks/iam-ondb-testset.txt: data/iam-ondb script/training.py
	script/training.py ondb_to_text data/iam-ondb/ $@ --subset testset_t

data/inks/iam-docdb-lines-%.txt: data/iam-docdb-1.0 script/training.py
	script/training.py docdb_to_text data/iam-docdb-1.0 $@ --subset $*.set

data/inks/iam-docdb-words-%.txt: data/iam-docdb-1.0 script/training.py
	script/training.py docdb_to_text data/iam-docdb-1.0 $@ --subset $*.set --split_words

data/inks/armrest.txt: data/inks/jabberwocky.txt data/inks/prufrock.txt data/inks/if-commands.txt data/inks/if-transcript.txt
	cat $^ > $@

data/inks/armrest-trainset.txt: data/inks/armrest.txt script/training.py
	script/training.py augment data/inks/armrest.txt data/inks/armrest-trainset.txt --subset trainset --target_size 3000

data/inks/armrest-validset.txt: data/inks/armrest.txt script/training.py
	script/training.py augment data/inks/armrest.txt data/inks/armrest-validset.txt --subset validset --target_size 1000

data/inks/armrest-testset.txt: data/inks/armrest.txt script/training.py
	script/training.py augment data/inks/armrest.txt data/inks/armrest-testset.txt --subset validset --target_size 1000

data/inks/trainset.txt: data/inks/iam-docdb-lines-0.txt data/inks/iam-docdb-words-0.txt data/inks/iam-docdb-lines-1.txt data/inks/iam-docdb-words-1.txt data/inks/iam-docdb-lines-2.txt data/inks/iam-docdb-words-2.txt data/inks/iam-ondb-trainset.txt data/inks/armrest-trainset.txt
	cat $^ > $@

data/inks/validset.txt: data/inks/iam-docdb-lines-3.txt data/inks/iam-docdb-words-3.txt data/inks/iam-ondb-validset.txt data/inks/armrest-validset.txt
	cat $^ > $@

data/inks/testset.txt: data/inks/iam-docdb-lines-4.txt data/inks/iam-docdb-words-4.txt data/inks/iam-ondb-testset.txt data/inks/armrest-testset.txt
	cat $^ > $@

data/tensors/%.txt: data/inks/%.txt src/
	mkdir -p data/tensors
	cargo run --release --bin ink-to-tensor spline < data/inks/$*.txt > data/tensors/$*.txt

data/model: script/training.py
	script/training.py create_keras data/model
