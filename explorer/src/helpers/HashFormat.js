import React, {Component} from 'react'

// LiveData recieves two props, socket and event
export default class HashFormat extends Component {
  constructor(props) {
    super(props)

    this.state = {
      shorthash: ''
    }

    if (this.props.hash)  {
      let hash = this.props.hash;
      let hashlength = this.props.hash.length;

      let start = hash.substring(0,4)
      let end = this.props.hash.substring((hashlength - 4), hashlength)

      let shorthash = ''.concat(start,'...',end)
      this.state.shorthash = shorthash
    }
  }

  componentDidMount() {

  }

  render() {

    return (
        <span>
        {this.state.shorthash}
      </span>
    )
  }

}
