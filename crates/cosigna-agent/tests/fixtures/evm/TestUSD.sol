// SPDX-License-Identifier: MIT
// A minimal 6-decimal ERC-20 for local test chains only: anyone can mint. Never deploy it to a
// network where its tokens could be mistaken for money.
pragma solidity 0.8.26;

contract TestUSD {
    string public constant name = "Test USD";
    string public constant symbol = "TUSD";
    uint8 public constant decimals = 6;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;

    event Transfer(address indexed from, address indexed to, uint256 value);

    function mint(address to, uint256 value) external {
        totalSupply += value;
        balanceOf[to] += value;
        emit Transfer(address(0), to, value);
    }

    function transfer(address to, uint256 value) external returns (bool) {
        require(balanceOf[msg.sender] >= value, "balance");
        balanceOf[msg.sender] -= value;
        balanceOf[to] += value;
        emit Transfer(msg.sender, to, value);
        return true;
    }
}
