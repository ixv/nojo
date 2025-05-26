run: nojo.jam
	cargo run

nojo.jam: ./hoon/apps/nojo.hoon
	hoonc --output ./nojo.jam ./hoon/apps/nojo.hoon

clean:
	rm nojo.jam
