// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.7.0;

contract flipper {
	bool private value;

	constructor(bool initvalue) {
		value = initvalue;
	}

	function flip() public {
		value = !value;
	}

	function get() public view returns (bool) {
		return value;
	}
}